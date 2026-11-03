//! Software update (NBP_CMD_UPDATE, nbp.h "software update"): the layouts
//! and checks; the app runs the steps (erase / write / verify the update
//! slot, then UPDATE). Mirrors the Instrix-Nebulosa-Debugger's board.cpp.
//!
//! The H417 flash holds two slots of `slot_size` (448 KB) from 0: the active
//! gateware in one, the other is the update slot. A gateware is tested
//! (loaded, DONE, the IP core's ID) before it's made active, else the old
//! one comes back; a firmware image (.nbfw) is checked and installed by the
//! bootloader at a restart.

use crate::le32;
use serde::Serialize;

pub const INFO: u8 = 0;
pub const GATEWARE: u8 = 1;
pub const FIRMWARE: u8 = 2;
pub const REVERT: u8 = 3;
pub const RECOVERY: u8 = 4;

pub const SLOT_EMPTY: u8 = 0;
pub const SLOT_GATEWARE: u8 = 1;
pub const SLOT_FIRMWARE: u8 = 2;

/// The board this firmware is for (NBP_BOARD_DIGITAL).
pub const BOARD_DIGITAL: u32 = 0x4E44_0001;

/// zlib's CRC-32.
pub fn crc32(p: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in p {
        c ^= b as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ if c & 1 != 0 { 0xEDB8_8320 } else { 0 };
        }
    }
    !c
}

/// nbp_update_t
pub fn enc_update(op: u8, slot: u8, len: u32, crc: u32) -> Vec<u8> {
    let mut v = vec![op, slot, 0, 0];
    v.extend_from_slice(&len.to_le_bytes());
    v.extend_from_slice(&crc.to_le_bytes());
    v
}

/// nbp_flash_t (+ data for FLASH_WRITE)
pub fn enc_flash(addr: u32, len: u32, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + data.len());
    v.extend_from_slice(&addr.to_le_bytes());
    v.extend_from_slice(&len.to_le_bytes());
    v.extend_from_slice(data);
    v
}

#[derive(Debug, Clone, Serialize)]
pub struct Slot {
    /// SLOT_*
    pub kind: u8,
    pub len: u32,
    pub crc: u32,
    /// gateware: the IP core's ID; firmware: its version
    pub id: u32,
}

/// nbp_update_info_t
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub fw_version: u32,
    pub fw_version_str: String,
    /// git commit, 8 hex digits
    pub fw_build_id: u32,
    pub fw_len: u32,
    pub fw_crc: u32,
    pub fw_date: String,
    /// 0: none
    pub boot_version: u32,
    pub active: u8,
    pub update_slot: u8,
    pub fw_pending: bool,
    /// NBP_ST_* of the operation
    pub result: u8,
    pub slot_size: u32,
    pub slots: [Slot; 2],
    pub installs: u32,
    pub msg: String,
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn ver(v: u32) -> String {
    format!("{}.{}.{}", v >> 16, (v >> 8) & 0xFF, v & 0xFF)
}

impl UpdateInfo {
    pub const LEN: usize = 16 + 24 + 4 + 4 + 4 + 2 * 16 + 4 + 48;

    pub fn parse(p: &[u8]) -> Option<UpdateInfo> {
        if p.len() < Self::LEN {
            return None;
        }
        let slot = |o: usize| Slot { kind: p[o], len: le32(p, o + 4), crc: le32(p, o + 8), id: le32(p, o + 12) };
        Some(UpdateInfo {
            fw_version: le32(p, 0),
            fw_version_str: ver(le32(p, 0)),
            fw_build_id: le32(p, 4),
            fw_len: le32(p, 8),
            fw_crc: le32(p, 12),
            fw_date: cstr(&p[16..40]),
            boot_version: le32(p, 40),
            active: p[44],
            update_slot: p[45],
            fw_pending: p[46] != 0,
            result: p[47],
            slot_size: le32(p, 48),
            slots: [slot(52), slot(68)],
            installs: le32(p, 84),
            msg: cstr(&p[88..136]),
        })
    }
}

/// A firmware image (.nbfw, src/update/update.h fw_header_t) checked: its
/// version, or what's wrong with it.
#[derive(Debug, Clone, Serialize)]
pub struct FirmwareCheck {
    pub ok: bool,
    pub version: u32,
    pub version_str: String,
    pub error: String,
}

pub fn firmware_check(img: &[u8]) -> FirmwareCheck {
    let bad = |e: &str| FirmwareCheck { ok: false, version: 0, version_str: String::new(), error: e.into() };
    if img.len() < 64 || le32(img, 0) != 0x5746_424E {
        return bad("not an .nbfw firmware image (no NBFW header)");
    }
    if u16::from_le_bytes([img[4], img[5]]) != 64 {
        return bad("unknown header size");
    }
    if crc32(&img[..60]) != le32(img, 60) {
        return bad("header CRC wrong");
    }
    if le32(img, 16) as usize != img.len() {
        return bad("the length in the header doesn't match the file");
    }
    if crc32(&img[64..]) != le32(img, 20) {
        return bad("image CRC wrong: the file is damaged");
    }
    if le32(img, 56) != BOARD_DIGITAL {
        return bad("an image for another board");
    }
    let v = le32(img, 24);
    FirmwareCheck { ok: true, version: v, version_str: ver(v), error: String::new() }
}

/// A gateware file to the raw bitstream the FPGA loads: a .bit's 'e' field,
/// or a .bin as it is. Err when there's no sync word (0xAA995566).
pub fn bitstream(d: &[u8]) -> Result<Vec<u8>, String> {
    const SYNC: [u8; 4] = [0xAA, 0x99, 0x55, 0x66];
    if !d.windows(4).any(|w| w == SYNC) {
        return Err("no sync word (0xAA995566): not a bitstream".into());
    }
    // .bit: fields 'a'..'d' (16-bit lengths), then 'e' with a 32-bit length
    if d.len() > 13 && d[0] == 0x00 && d[1] == 0x09 {
        let mut p = 2 + 9 + 2;
        while p < d.len() {
            let key = d[p];
            p += 1;
            if key == b'e' {
                if p + 4 > d.len() {
                    break;
                }
                let n = u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize;
                p += 4;
                if p + n > d.len() {
                    break;
                }
                return Ok(d[p..p + n].to_vec());
            }
            if p + 2 > d.len() {
                break;
            }
            p += 2 + ((d[p] as usize) << 8 | d[p + 1] as usize);
        }
        return Err("malformed .bit header".into());
    }
    Ok(d.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_is_zlibs() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    fn nbfw(body: &[u8], board: u32) -> Vec<u8> {
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(&0x5746_424Eu32.to_le_bytes());
        h[4..6].copy_from_slice(&64u16.to_le_bytes());
        h[16..20].copy_from_slice(&((64 + body.len()) as u32).to_le_bytes());
        h[20..24].copy_from_slice(&crc32(body).to_le_bytes());
        h[24..28].copy_from_slice(&0x0002_0300u32.to_le_bytes());
        h[56..60].copy_from_slice(&board.to_le_bytes());
        let c = crc32(&h[..60]);
        h[60..64].copy_from_slice(&c.to_le_bytes());
        h.extend_from_slice(body);
        h
    }

    #[test]
    fn firmware_images() {
        let ok = firmware_check(&nbfw(&[1, 2, 3, 4], BOARD_DIGITAL));
        assert!(ok.ok, "{}", ok.error);
        assert_eq!(ok.version_str, "2.3.0");
        let mut damaged = nbfw(&[1, 2, 3, 4], BOARD_DIGITAL);
        *damaged.last_mut().unwrap() ^= 1;
        assert!(firmware_check(&damaged).error.contains("damaged"));
        assert!(firmware_check(&nbfw(&[1], 0x1234)).error.contains("another board"));
        assert!(firmware_check(b"hello").error.contains("NBFW"));
    }

    #[test]
    fn bit_and_bin() {
        let raw = [0xFF, 0xFF, 0xAA, 0x99, 0x55, 0x66, 0x30, 0x01];
        assert_eq!(bitstream(&raw).unwrap(), raw);
        // a .bit: header, 'a' "x", 'b' "p", then 'e' and the raw bits
        let mut bit = vec![0x00, 0x09, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x00, 0x00, 0x01];
        bit.extend_from_slice(&[b'a', 0, 1, b'x', b'b', 0, 1, b'p', b'e']);
        bit.extend_from_slice(&(raw.len() as u32).to_be_bytes());
        bit.extend_from_slice(&raw);
        assert_eq!(bitstream(&bit).unwrap(), raw);
        assert!(bitstream(&[0, 1, 2, 3]).is_err());
    }

    #[test]
    fn info_layout() {
        let mut p = vec![0u8; UpdateInfo::LEN];
        p[0..4].copy_from_slice(&0x0002_0000u32.to_le_bytes());
        p[16..21].copy_from_slice(b"Nov 3");
        p[44] = 0;
        p[45] = 1;
        p[48..52].copy_from_slice(&(448u32 * 1024).to_le_bytes());
        p[68] = SLOT_FIRMWARE;
        p[88..92].copy_from_slice(b"done");
        let i = UpdateInfo::parse(&p).unwrap();
        assert_eq!((i.fw_version_str.as_str(), i.update_slot, i.slot_size), ("2.0.0", 1, 448 * 1024));
        assert_eq!(i.slots[1].kind, SLOT_FIRMWARE);
        assert_eq!((i.fw_date.as_str(), i.msg.as_str()), ("Nov 3", "done"));
    }
}

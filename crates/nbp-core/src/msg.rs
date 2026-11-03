//! Command codes, status codes and payload structs (nbp.h).

use crate::{le16, le32};
use serde::Serialize;

pub const VERSION: u32 = 1;

pub const USB_VID: u16 = 0x1A86;
pub const USB_PID_HS: u16 = 0xFE0D; // High-Speed and SuperSpeed
pub const USB_PID_FS: u16 = 0xFE0C;
pub const USB_CLASS: u8 = 0xFF;
pub const USB_SUBCLASS: u8 = 0x4E;
pub const USB_PROTOCOL: u8 = 0x01;
pub const TCP_PORT: u16 = 20034;
pub const UDP_DISCOVERY_PORT: u16 = 20035;

pub const DATA: u8 = 0x80;
pub const fn rsp(cmd: u8) -> u8 {
    cmd | 0x40
}

pub mod cmd {
    pub const PING: u8 = 0x01;
    pub const GET_INFO: u8 = 0x02;
    pub const REG_READ: u8 = 0x03;
    pub const REG_WRITE: u8 = 0x04;
    pub const RESET: u8 = 0x05;
    pub const STREAM_START: u8 = 0x10;
    pub const STREAM_STOP: u8 = 0x11;
    pub const GET_STATS: u8 = 0x12;
    pub const SET_SOURCE: u8 = 0x20;
    pub const SET_RANGE: u8 = 0x21;
    pub const GET_RANGE: u8 = 0x22;
    pub const SET_RANGE_TIMING: u8 = 0x23;
    pub const SET_RANGE_FORCE: u8 = 0x24;
    pub const SET_DECIMATION: u8 = 0x30;
    pub const BOARD_STATUS: u8 = 0x38;
    pub const BOARD_SET: u8 = 0x39;
    pub const BOARD_TEST: u8 = 0x3A;
    pub const FLASH_READ: u8 = 0x3B;
    pub const FLASH_WRITE: u8 = 0x3C;
    pub const FLASH_ERASE: u8 = 0x3D;
    pub const FPGA_CONFIG: u8 = 0x3E;
    pub const UPDATE: u8 = 0x3F;
}

pub mod status {
    pub const OK: u8 = 0;
    pub const UNKNOWN: u8 = 1;
    pub const BAD_LEN: u8 = 2;
    pub const BAD_ARG: u8 = 3;
    pub const FPGA: u8 = 4;
    pub const BUSY: u8 = 5;
    pub const HW: u8 = 6;
    pub const NO_POWER: u8 = 7;
    pub const PROTECTED: u8 = 8;

    pub fn name(s: u8) -> &'static str {
        match s {
            OK => "OK",
            UNKNOWN => "UNKNOWN",
            BAD_LEN => "BAD_LEN",
            BAD_ARG => "BAD_ARG",
            FPGA => "FPGA",
            BUSY => "BUSY",
            HW => "HW",
            NO_POWER => "NO_POWER",
            PROTECTED => "PROTECTED",
            _ => "?",
        }
    }
}

pub mod link {
    pub const NONE: u32 = 0;
    pub const USB_FS: u32 = 1;
    pub const USB_HS: u32 = 2;
    pub const TCP: u32 = 3;
    pub const USB_SS: u32 = 4;

    pub fn name(l: u32) -> &'static str {
        match l {
            USB_FS => "USB Full-Speed",
            USB_HS => "USB High-Speed",
            TCP => "TCP",
            USB_SS => "USB SuperSpeed",
            _ => "none",
        }
    }
}

pub mod caps {
    pub const USB_HS: u32 = 1 << 0;
    pub const RANGE: u32 = 1 << 1;
    pub const DECIM: u32 = 1 << 2;
    pub const ETH: u32 = 1 << 3;
    pub const BOARD: u32 = 1 << 4;
    pub const USB_SS: u32 = 1 << 5;
}

pub const START_FLUSH: u32 = 1 << 0;
pub const START_LINK_TEST: u32 = 1 << 1;

pub const SRC_STUB_RANDOM: u8 = 0;
pub const SRC_STUB_PATTERN: u8 = 1;
pub const SRC_AD7387: u8 = 2;

pub const DECIM_AVERAGE: u8 = 0;
pub const DECIM_SUBSAMPLE: u8 = 1;

/// First FPGA ID with stream format 5.
pub const FPGA_ID_FORMAT5: u32 = 0x4E42_000B;

fn version_str(v: u32) -> String {
    format!("{}.{}.{}", v >> 16, (v >> 8) & 0xFF, v & 0xFF)
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub proto_version: u32,
    pub fw_version: u32,
    pub fw_version_str: String,
    pub fpga_id: u32,
    pub base_rate_hz: u32,
    pub link: u32,
    pub link_name: String,
    pub link_unit: u32,
    pub block_words: u32,
    pub caps: u32,
    pub build: String,
    /// "aa:bb:..", empty without Ethernet
    pub mac: String,
    /// dotted IPv4, empty without one
    pub ip: String,
}

impl Info {
    pub const LEN: usize = 32 + 32 + 6 + 2 + 4;

    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < Self::LEN {
            return None;
        }
        let mac = &p[64..70];
        let ip = &p[72..76];
        let link = le32(p, 16);
        Some(Self {
            proto_version: le32(p, 0),
            fw_version: le32(p, 4),
            fw_version_str: version_str(le32(p, 4)),
            fpga_id: le32(p, 8),
            base_rate_hz: le32(p, 12),
            link,
            link_name: link::name(link).into(),
            link_unit: le32(p, 20),
            block_words: le32(p, 24),
            caps: le32(p, 28),
            build: cstr(&p[32..64]),
            mac: if mac.iter().all(|&b| b == 0) {
                String::new()
            } else {
                mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
            },
            // network byte order
            ip: if ip.iter().all(|&b| b == 0) {
                String::new()
            } else {
                format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
            },
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub uptime_ms: u32,
    pub stream_link: u32,
    pub blocks: u32,
    pub words: u32,
    pub stalls: u32,
    pub fpga_drops: u32,
    pub fifo_level: u32,
    pub range_status: u32,
    pub decim: u32,
    pub adc_ctrl: u32,
    pub eth: u32,
    pub eth_ip: u32,
}

impl Stats {
    pub const LEN: usize = 16 * 4;

    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < Self::LEN {
            return None;
        }
        let w = |i: usize| le32(p, i * 4);
        Some(Self {
            uptime_ms: w(0),
            stream_link: w(1),
            blocks: w(2),
            words: w(3),
            stalls: w(4),
            fpga_drops: w(5),
            fifo_level: w(6),
            range_status: w(7),
            decim: w(8),
            adc_ctrl: w(9),
            eth: w(10),
            eth_ip: w(11),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Decim {
    pub log2n: u8,
    pub mode: u8,
    pub rate_hz: u32,
}

impl Decim {
    pub fn parse(p: &[u8]) -> Option<Self> {
        (p.len() >= 8).then(|| Self { log2n: p[0], mode: p[1], rate_hz: le32(p, 4) })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeState {
    pub status: u32,
    pub count: u32,
    pub ctrl: u32,
    pub timing: u32,
}

impl RangeState {
    pub fn parse(p: &[u8]) -> Option<Self> {
        (p.len() >= 16).then(|| Self {
            status: le32(p, 0),
            count: le32(p, 4),
            ctrl: le32(p, 8),
            timing: le32(p, 12),
        })
    }
}

/// DATA block header (`nbp_block_t` without the words).
#[derive(Debug, Clone, Copy)]
pub struct BlockHdr {
    pub seq: u32,
    pub t_us: u32,
    pub nwords: u16,
    pub flags: u16,
}

pub const BLK_FIRST: u16 = 1 << 0;
pub const BLK_STALL: u16 = 1 << 1;
pub const BLK_GAP: u16 = 1 << 2;
pub const BLK_TEST: u16 = 1 << 3;
pub const BLOCK_WORDS: usize = 507;

impl BlockHdr {
    pub const LEN: usize = 12;

    pub fn parse(p: &[u8]) -> Option<Self> {
        (p.len() >= Self::LEN).then(|| Self {
            seq: le32(p, 0),
            t_us: le32(p, 4),
            nwords: le16(p, 8),
            flags: le16(p, 10),
        })
    }
}

// ---- payload encoders -------------------------------------------------------

pub fn enc_stream_start(flags: u32) -> Vec<u8> {
    flags.to_le_bytes().to_vec()
}

pub fn enc_source(src: u8) -> Vec<u8> {
    vec![src, 0, 0, 0]
}

pub fn enc_decim(log2n: u8, mode: u8) -> Vec<u8> {
    vec![log2n, mode, 0, 0, 0, 0, 0, 0]
}

pub fn enc_range_set(automatic: bool, manual: u8, min: u8, max: u8) -> Vec<u8> {
    vec![automatic as u8, manual, min, max]
}

pub fn enc_reg(index: u8, value: u32) -> Vec<u8> {
    let mut v = vec![index, 0, 0, 0];
    v.extend_from_slice(&value.to_le_bytes());
    v
}

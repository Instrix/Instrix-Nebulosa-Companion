//! Board commands (NBP_CAP_BOARD, nbp.h "board commands"): the board's
//! status (inputs, USB PD, analog supplies, clock, SD card, FPGA), its
//! settings and its self-tests.

use crate::{le16, le32};
use serde::Serialize;

pub const CMD_STATUS: u8 = 0x38;
pub const CMD_SET: u8 = 0x39;
pub const CMD_TEST: u8 = 0x3A;

/// NBP_BSET_*
pub mod set {
    pub const LEDS: u8 = 0;
    pub const APWR_H: u8 = 1;
    pub const APWR_L: u8 = 2;
    pub const USB3_EN: u8 = 3;
    pub const USB3_DIR: u8 = 4;
    pub const PWR_SEL: u8 = 5;
    pub const FPGA_PROGRAM: u8 = 6;
    pub const LCD: u8 = 7;
    pub const RTC: u8 = 8;
    pub const PD_MV: u8 = 9;
    pub const UI_KEY: u8 = 10;
}

/// nbp_board_status_t.flags
pub mod flag {
    pub const HSE: u32 = 1 << 0;
    pub const LSE: u32 = 1 << 1;
    pub const RTC: u32 = 1 << 2;
    pub const DFU: u32 = 1 << 3;
    pub const SD_CARD: u32 = 1 << 4;
    pub const PWR_ST: u32 = 1 << 5;
    pub const APWR_H: u32 = 1 << 6;
    pub const APWR_L: u32 = 1 << 7;
    pub const USB3_EN: u32 = 1 << 8;
    pub const USB3_DIR: u32 = 1 << 9;
    pub const FPGA_DONE: u32 = 1 << 10;
    pub const FPGA_INIT_B: u32 = 1 << 11;
    pub const FPGA_PROG_HELD: u32 = 1 << 12;
    pub const USB_HS: u32 = 1 << 13;
    pub const ETH_LINK: u32 = 1 << 14;
    pub const LCD: u32 = 1 << 15;
    pub const SWD: u32 = 1 << 16;
    pub const USB3_AUTO: u32 = 1 << 17;
    pub const USB3_EN_AUTO: u32 = 1 << 18;
    pub const USB_C: u32 = 1 << 19;
    pub const APWR_OK: u32 = 1 << 20;
    pub const USB_SS: u32 = 1 << 21;
}

/// NBP_TEST_*, in bit order: (name, what it checks, what its detail is)
pub const TESTS: [(&str, &str, &str); 12] = [
    ("HSE", "25 MHz crystal, system clock from it", "HCLK Hz"),
    ("LSE", "32.768 kHz crystal starts", "start-up ms"),
    ("RTC", "RTC counts on the LSE", "LSE Hz"),
    ("Rails", "V_MAIN in range, an input present", "V_MAIN mV"),
    ("Flash ID", "SPI flash answers its JEDEC ID", "ID"),
    ("Flash R/W", "erase / program / read back a sector", "bad bytes"),
    ("SD card", "card identifies, block 0 reads", "error step"),
    ("LCD", "LCD initialised (look at it)", ""),
    ("FPGA DONE", "FPGA configured", ""),
    ("FPGA QSPI", "FPGA ID register over QSPI", "ID"),
    ("Ethernet PHY", "PHY answers", "PHY ID"),
    ("USB PD", "PD contract at the target", "contract mV"),
];
pub const TEST_ALL: u32 = (1 << TESTS.len()) - 1;

#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub name: &'static str,
    pub what: &'static str,
    /// "none" (not run), "pass", "fail", "skip"
    pub result: &'static str,
    pub detail: u32,
    pub detail_unit: &'static str,
}

/// A USB PD source capability (a fixed supply PDO; others have mv = 0).
#[derive(Debug, Clone, Serialize)]
pub struct Pdo {
    pub mv: u32,
    pub ma: u32,
    pub fixed: bool,
}

/// nbp_board_status_t
#[derive(Debug, Clone, Serialize)]
pub struct BoardStatus {
    pub board_id: u32,
    pub uptime_ms: u32,
    pub core_hz: u32,
    pub hclk_hz: u32,
    pub flags: u32,
    pub rtc_s: u32,
    pub v_bj_mv: u16,
    pub v_vbus_mv: u16,
    pub v_main_mv: u16,
    pub leds: u8,
    pub led_manual: bool,
    /// 0 auto, 1 barrel jack only, 2 USB only
    pub pwr_sel: u8,
    pub cc: u8,
    /// 0 unknown, 1 master (own flash), 2 slave (loaded by the H417)
    pub fpga_mode: u8,
    /// 0 off, 1 status UI, 2 test pattern
    pub lcd_mode: u8,
    /// 0 none, 1 SDSC v1, 2 SDSC v2, 3 SDHC / SDXC
    pub sd_type: u8,
    pub fpga_cfg_ms: u32,
    pub fpga_cfg_bytes: u32,
    pub fpga_id: u32,
    pub flash_jedec: u32,
    pub flash_bytes: u32,
    pub sd_blocks: u32,
    pub tests: Vec<TestResult>,
    /// NBP_PD_*: 0 detached, 1..3 negotiating, 4 contract, 5 no PD
    pub pd_state: u8,
    pub pd_cc_line: u8,
    pub pd_spec_rev: u8,
    pub pd_mv: u16,
    pub pd_ma: u16,
    pub pd_target_mv: u16,
    pub pd_pdo_sel: u8,
    pub pd_pdos: Vec<Pdo>,
    pub pd_contracts: u32,
    pub pd_hard_resets: u32,
    pub pd_errors: u32,
    // the flags, spelled out
    pub apwr_h: bool,
    pub apwr_l: bool,
    /// a 12 V source feeds V_MAIN: the analog supplies may be switched on
    pub apwr_ok: bool,
    pub usb_c: bool,
    pub usb_hs: bool,
    pub usb_ss: bool,
    pub eth_link: bool,
    pub sd_card: bool,
    pub rtc_running: bool,
    pub fpga_done: bool,
}

impl BoardStatus {
    pub const LEN: usize = 200;

    pub fn parse(p: &[u8]) -> Option<BoardStatus> {
        if p.len() < Self::LEN {
            return None;
        }
        let flags = le32(p, 16);
        let results = le32(p, 80);
        let tests = TESTS
            .iter()
            .enumerate()
            .map(|(k, &(name, what, unit))| TestResult {
                name,
                what,
                result: ["none", "pass", "fail", "skip"][((results >> (2 * k)) & 3) as usize],
                detail: le32(p, 84 + 4 * k),
                detail_unit: unit,
            })
            .collect();
        let npdo = (p[135] as usize).min(7);
        let pd_pdos = (0..npdo)
            .map(|k| {
                let w = le32(p, 144 + 4 * k);
                let fixed = w >> 30 == 0;
                Pdo { mv: if fixed { ((w >> 10) & 0x3FF) * 50 } else { 0 }, ma: if fixed { (w & 0x3FF) * 10 } else { 0 }, fixed }
            })
            .collect();
        Some(BoardStatus {
            board_id: le32(p, 0),
            uptime_ms: le32(p, 4),
            core_hz: le32(p, 8),
            hclk_hz: le32(p, 12),
            flags,
            rtc_s: le32(p, 20),
            v_bj_mv: le16(p, 24),
            v_vbus_mv: le16(p, 26),
            v_main_mv: le16(p, 28),
            leds: p[32],
            led_manual: p[33] != 0,
            pwr_sel: p[34],
            cc: p[35],
            fpga_mode: p[36],
            lcd_mode: p[37],
            sd_type: p[38],
            fpga_cfg_ms: le32(p, 40),
            fpga_cfg_bytes: le32(p, 44),
            fpga_id: le32(p, 48),
            flash_jedec: le32(p, 52),
            flash_bytes: le32(p, 56),
            sd_blocks: le32(p, 60),
            tests,
            pd_state: p[132],
            pd_cc_line: p[133],
            pd_spec_rev: p[134],
            pd_mv: le16(p, 136),
            pd_ma: le16(p, 138),
            pd_target_mv: le16(p, 140),
            pd_pdo_sel: p[142],
            pd_pdos,
            pd_contracts: le32(p, 172),
            pd_hard_resets: le32(p, 176),
            pd_errors: le32(p, 180),
            apwr_h: flags & flag::APWR_H != 0,
            apwr_l: flags & flag::APWR_L != 0,
            apwr_ok: flags & flag::APWR_OK != 0,
            usb_c: flags & flag::USB_C != 0,
            usb_hs: flags & flag::USB_HS != 0,
            usb_ss: flags & flag::USB_SS != 0,
            eth_link: flags & flag::ETH_LINK != 0,
            sd_card: flags & flag::SD_CARD != 0,
            rtc_running: flags & flag::RTC != 0,
            fpga_done: flags & flag::FPGA_DONE != 0,
        })
    }
}

/// nbp_board_set_t
pub fn enc_set(item: u8, value: u32) -> Vec<u8> {
    let mut v = vec![item, 0, 0, 0];
    v.extend_from_slice(&value.to_le_bytes());
    v
}

/// nbp_board_test_t
pub fn enc_test(mask: u32) -> Vec<u8> {
    (mask & TEST_ALL).to_le_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_layout() {
        let mut p = vec![0u8; BoardStatus::LEN];
        p[0..4].copy_from_slice(&0x4E44_0001u32.to_le_bytes());
        p[16..20].copy_from_slice(&(flag::APWR_OK | flag::APWR_H | flag::USB_SS).to_le_bytes());
        p[24..26].copy_from_slice(&12_050u16.to_le_bytes());
        p[28..30].copy_from_slice(&11_900u16.to_le_bytes());
        // HSE pass, LSE fail, SD skip
        p[80..84].copy_from_slice(&(1u32 | 2 << 2 | 3 << 12).to_le_bytes());
        p[84..88].copy_from_slice(&150_000_000u32.to_le_bytes());
        p[132] = 4;
        p[135] = 2;
        p[136..138].copy_from_slice(&12_000u16.to_le_bytes());
        // fixed 5 V 3 A, fixed 12 V 2.25 A
        p[144..148].copy_from_slice(&((100u32 << 10) | 300).to_le_bytes());
        p[148..152].copy_from_slice(&((240u32 << 10) | 225).to_le_bytes());
        p[180..184].copy_from_slice(&7u32.to_le_bytes());
        let s = BoardStatus::parse(&p).unwrap();
        assert!(s.apwr_ok && s.apwr_h && !s.apwr_l && s.usb_ss);
        assert_eq!((s.v_bj_mv, s.v_main_mv), (12_050, 11_900));
        assert_eq!((s.tests[0].result, s.tests[1].result, s.tests[6].result, s.tests[2].result), ("pass", "fail", "skip", "none"));
        assert_eq!(s.tests[0].detail, 150_000_000);
        assert_eq!((s.pd_state, s.pd_mv, s.pd_errors), (4, 12_000, 7));
        assert_eq!((s.pd_pdos[0].mv, s.pd_pdos[0].ma, s.pd_pdos[1].mv, s.pd_pdos[1].ma), (5000, 3000, 12_000, 2250));
        assert!(BoardStatus::parse(&p[..199]).is_none());
    }

    #[test]
    fn encodings() {
        assert_eq!(enc_set(set::RTC, 0x0102_0304), vec![8, 0, 0, 0, 4, 3, 2, 1]);
        assert_eq!(enc_test(u32::MAX), TEST_ALL.to_le_bytes().to_vec());
    }
}

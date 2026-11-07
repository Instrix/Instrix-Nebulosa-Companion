//! The low-level side of the device, for the Debug tab (a build option):
//! the FPGA register map, the range switch's timing and forced comparators,
//! the RS1473 ADC link's status registers, raw flash access. Everything
//! here goes through REG_READ / REG_WRITE and the range / flash commands,
//! the same as the Nebulosa Debugger (Instrix-Nebulosa-Debugger) does.

use serde::Serialize;

use crate::le32;

/// registers 0..44 (42..44: the RS1473 link, gateware 0x4E42000A on)
pub const FPGA_REGS: u8 = 45;

pub mod reg {
    pub const ADC_CTRL: u8 = 31;
    pub const FIFO_LEVEL: u8 = 32;
    pub const DROP_COUNT: u8 = 33;
    pub const ID: u8 = 34;
    pub const RANGE_STATUS: u8 = 35;
    pub const RANGE_COUNT: u8 = 36;
    pub const RANGE_CTRL: u8 = 37;
    pub const RANGE_TIMING: u8 = 38;
    pub const DECIM: u8 = 39;
    pub const RANGE_CFG: u8 = 40;
    pub const CLK_HZ: u8 = 41;
    pub const ADC_TIMING: u8 = 42;
    pub const ADC_DELAY: u8 = 43;
    pub const ADC_ERRORS: u8 = 44;
}

/// first gateware with CLK_HZ; older ones run at 108 MHz
pub const FPGA_ID_CLK_HZ: u32 = 0x4E42_0006;
pub const FPGA_CLK_HZ_OLD: u32 = 108_000_000;
/// first gateware with the RS1473 link registers
pub const FPGA_ID_RS1473: u32 = 0x4E42_000A;

/// a register's name ("" for the calibration block 0..30 and the unused ones)
pub fn reg_name(i: u8) -> &'static str {
    match i {
        0..=30 => "cal",
        reg::ADC_CTRL => "ADC_CTRL",
        reg::FIFO_LEVEL => "FIFO_LEVEL",
        reg::DROP_COUNT => "DROP_COUNT",
        reg::ID => "ID",
        reg::RANGE_STATUS => "RANGE_STATUS",
        reg::RANGE_COUNT => "RANGE_COUNT",
        reg::RANGE_CTRL => "RANGE_CTRL",
        reg::RANGE_TIMING => "RANGE_TIMING",
        reg::DECIM => "DECIM",
        reg::RANGE_CFG => "RANGE_CFG",
        reg::CLK_HZ => "CLK_HZ",
        reg::ADC_TIMING => "ADC_TIMING",
        reg::ADC_DELAY => "ADC_DELAY",
        reg::ADC_ERRORS => "ADC_ERRORS",
        _ => "",
    }
}

/// REG_READ's response (`nbp_reg_t`): the value
pub fn parse_reg(p: &[u8]) -> Option<u32> {
    (p.len() >= 8).then(|| le32(p, 4))
}

// ---- range switch -------------------------------------------------------------

/// `nbp_range_timing_t`; FPGA clk cycles
pub fn enc_range_timing(up_filt: u8, down_filt: u8, overlap: u8, hold: u16) -> Vec<u8> {
    let mut v = vec![up_filt & 0x3F, down_filt & 0x3F, overlap, 0];
    v.extend_from_slice(&(hold & 0xFFF).to_le_bytes());
    v.extend_from_slice(&[0, 0]);
    v
}

/// `nbp_range_force_t`: comparator bits {down_c, up_c, down_b, up_b, down_a, up_a}
pub fn enc_range_force(enable: bool, value: u8) -> Vec<u8> {
    vec![enable as u8, value & 0x3F, 0, 0]
}

/// RANGE_STATUS, RANGE_CTRL, RANGE_TIMING and RANGE_CFG spelled out
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RangeDecoded {
    /// the range in use, 0..5
    pub range: u8,
    pub switching: bool,
    /// EN_TAP outputs, bit per range
    pub taps: u8,
    /// comparators {up_a, down_a, up_b, down_b, up_c, down_c}
    pub comps: u8,
    pub automatic: bool,
    pub forced: bool,
    pub manual: u8,
    pub min: u8,
    pub max: u8,
    pub up_filt: u8,
    pub down_filt: u8,
    pub overlap: u8,
    pub hold: u16,
    pub settle: u16,
    pub align: bool,
    pub dirty_up: bool,
    pub dirty_down: bool,
    /// the raw comparator pins (RANGE_STATUS 29:24), same order as `comps`
    pub pins: u8,
}

pub fn decode_range(status: u32, ctrl: u32, timing: u32, cfg: u32) -> RangeDecoded {
    RangeDecoded {
        range: (status & 7) as u8,
        switching: status >> 3 & 1 != 0,
        taps: (status >> 8 & 0x3F) as u8,
        comps: (status >> 16 & 0x3F) as u8,
        automatic: ctrl & 1 != 0,
        forced: ctrl & 2 != 0,
        manual: ((ctrl >> 4) & 7).min(5) as u8,
        min: (ctrl >> 8 & 7) as u8,
        max: (ctrl >> 12 & 7) as u8,
        up_filt: (timing & 0x3F) as u8,
        down_filt: (timing >> 6 & 0x3F) as u8,
        overlap: (timing >> 12 & 0xFF) as u8,
        hold: (timing >> 20 & 0xFFF) as u16,
        settle: (cfg & 0xFFF) as u16,
        align: cfg >> 16 & 1 != 0,
        dirty_up: cfg >> 17 & 1 != 0,
        dirty_down: cfg >> 18 & 1 != 0,
        pins: (status >> 24 & 0x3F) as u8,
    }
}

/// RANGE_CFG from its fields
pub fn enc_range_cfg(settle: u16, align: bool, dirty_up: bool, dirty_down: bool) -> u32 {
    (settle as u32 & 0xFFF) | (align as u32) << 16 | (dirty_up as u32) << 17 | (dirty_down as u32) << 18
}

// ---- RS1473 ADC link ------------------------------------------------------------

pub mod adc_ctrl {
    pub const PATTERN: u32 = 1 << 0;
    pub const SRC_ADC: u32 = 1 << 1;
    pub const HOLD: u32 = 1 << 2;
    /// 0 -> 1: measure the eye again
    pub const RECAL: u32 = 1 << 3;
    /// phases from bits 7:5 (A) / 10:8 (B)
    pub const MANUAL: u32 = 1 << 4;
    pub const PHASES: u32 = 0x7E0;
    /// echo-clocked capture engine
    pub const ECHO: u32 = 1 << 11;
    pub const DIAG: u32 = 0xFF << 16;
}

/// ADC_CTRL's phase field for groups A, B (0..4, 5 ns each)
pub fn adc_phases(a: u8, b: u8) -> u32 {
    ((a as u32 & 7) << 5) | ((b as u32 & 7) << 8)
}

pub const ADC_PHASES: u8 = 5;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdcGroup {
    /// sample phase in use, 0..4
    pub phase: u8,
    pub locked: bool,
    /// bit k: a line changed between phase k-1 and k (last window)
    pub tmap: u8,
    /// the eye spans eye + 1 samples of 5 ns
    pub eye: u8,
    /// clk_fast engine: launch -> CS_RET seen low, 200 MHz cycles
    pub delay: u8,
    /// echo engine: the group's DCM
    pub dcm_locked: bool,
    pub echo_map: u8,
    /// phases where a sample reads a settled bit (5-bit mask)
    pub clean: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdcLink {
    pub ctrl: u32,
    pub timing: u32,
    pub delay: u32,
    pub errors: u32,
    /// ADC_TIMING bit 31: manual phases in effect
    pub manual: bool,
    pub echo: bool,
    pub hold: bool,
    /// the FIFO source: 0 stub random, 1 stub pattern, 2 the ADCs
    pub source: u8,
    pub groups: [AdcGroup; 2],
    pub locked: bool,
}

fn clean_phases(tmap: u8) -> u8 {
    let mut m = 0;
    for k in 0..ADC_PHASES {
        let a = tmap >> k & 1 != 0;
        let b = tmap >> ((k + 1) % ADC_PHASES) & 1 != 0;
        if !(a && b) {
            m |= 1 << k;
        }
    }
    m
}

pub fn decode_adc(ctrl: u32, timing: u32, delay: u32, errors: u32) -> AdcLink {
    let echo = ctrl & adc_ctrl::ECHO != 0;
    let group = |k: u32| {
        let t = timing >> (16 * k);
        let d = delay >> (8 * k);
        let tmap = (t >> 4 & 0x1F) as u8;
        AdcGroup {
            phase: (t & 7) as u8,
            locked: t >> 3 & 1 != 0,
            tmap,
            eye: (t >> 9 & 7) as u8,
            delay: if echo { 0 } else { (d & 0x7F) as u8 },
            dcm_locked: echo && d >> 7 & 1 != 0,
            echo_map: (delay >> (16 + 8 * k) & 0x1F) as u8,
            clean: clean_phases(tmap),
        }
    };
    let groups = [group(0), group(1)];
    let locked = groups.iter().all(|g| g.locked && (!echo || g.dcm_locked));
    AdcLink {
        ctrl,
        timing,
        delay,
        errors,
        manual: timing >> 31 & 1 != 0,
        echo,
        hold: ctrl & adc_ctrl::HOLD != 0,
        source: (ctrl & 3) as u8,
        groups,
        locked,
    }
}

// ---- probe: raw codes of one range ------------------------------------------------

/// Welford's running mean / variance
#[derive(Debug, Clone, Copy, Default)]
struct Run {
    n: u64,
    mean: f64,
    m2: f64,
}

impl Run {
    fn add(&mut self, x: f64) {
        self.n += 1;
        let d = x - self.mean;
        self.mean += d / self.n as f64;
        self.m2 += d * (x - self.mean);
    }
    fn sd(&self) -> f64 {
        if self.n > 1 { (self.m2 / (self.n - 1) as f64).sqrt() } else { 0.0 }
    }
}

/// The raw current (and voltage) codes of the samples taken in one range,
/// clean and DIRTY apart: what the bring-up measures a tap with (the
/// debugger's adc::Collector).
#[derive(Debug, Clone)]
pub struct Probe {
    range: Option<u8>,
    clean: Run,
    all: Run,
    volt: Run,
    other: u64,
    min: u16,
    max: u16,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProbeResult {
    /// clean samples
    pub n: u64,
    /// DIRTY samples of the range
    pub dirty: u64,
    /// samples of other ranges, skipped
    pub other: u64,
    /// current code: mean and rms of the clean samples (of all of them when none is clean)
    pub mean: f64,
    pub sd: f64,
    pub min: u16,
    pub max: u16,
    /// the mean voltage code
    pub v_mean: f64,
    /// no clean sample: mean / sd are over DIRTY ones
    pub all_dirty: bool,
}

impl Probe {
    pub fn new(range: Option<u8>) -> Self {
        Self { range, clean: Run::default(), all: Run::default(), volt: Run::default(), other: 0, min: u16::MAX, max: 0 }
    }

    pub fn sample(&mut self, ic: u16, vc: u16, range: u8, dirty: bool) {
        if self.range.is_some_and(|r| r != range) {
            self.other += 1;
            return;
        }
        self.all.add(ic as f64);
        self.volt.add(vc as f64);
        if !dirty {
            self.clean.add(ic as f64);
            self.min = self.min.min(ic);
            self.max = self.max.max(ic);
        }
    }

    pub fn result(&self) -> ProbeResult {
        let all_dirty = self.clean.n == 0;
        let r = if all_dirty { &self.all } else { &self.clean };
        ProbeResult {
            n: self.clean.n,
            dirty: self.all.n - self.clean.n,
            other: self.other,
            mean: r.mean,
            sd: r.sd(),
            min: if self.clean.n > 0 { self.min } else { 0 },
            max: self.max,
            v_mean: self.volt.mean,
            all_dirty,
        }
    }
}

// ---- flash ------------------------------------------------------------------

/// FLASH_READ's response: `nbp_flash_t`, then the bytes
pub fn parse_flash_read(p: &[u8]) -> Option<Vec<u8>> {
    if p.len() < 8 {
        return None;
    }
    let len = le32(p, 4) as usize;
    p.get(8..8 + len).map(|d| d.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_timing_layout() {
        assert_eq!(enc_range_timing(2, 16, 26, 27), vec![2, 16, 26, 0, 27, 0, 0, 0]);
        assert_eq!(enc_range_timing(63, 63, 255, 4095), vec![63, 63, 255, 0, 0xFF, 0x0F, 0, 0]);
        assert_eq!(enc_range_force(true, 0x15), vec![1, 0x15, 0, 0]);
    }

    #[test]
    fn range_round_trip() {
        let timing = 2 | 16 << 6 | 26 << 12 | 27 << 20;
        let cfg = enc_range_cfg(40, true, false, true);
        let r = decode_range(3 | 1 << 3 | 0b001000 << 8 | 0b10 << 16, 1 | 2 << 8 | 5 << 12, timing, cfg);
        assert_eq!((r.range, r.switching, r.taps, r.comps), (3, true, 8, 2));
        assert!(r.automatic && !r.forced);
        assert_eq!((r.min, r.max), (2, 5));
        assert_eq!((r.up_filt, r.down_filt, r.overlap, r.hold), (2, 16, 26, 27));
        assert_eq!((r.settle, r.align, r.dirty_up, r.dirty_down), (40, true, false, true));
    }

    #[test]
    fn adc_link_decode() {
        // group A: phase 2, locked, map 0b10001, eye 3; group B: phase 4, unlocked
        let ta = 2 | 1 << 3 | 0b10001 << 4 | 3 << 9;
        let tb = 4 | 0b00110 << 4;
        let l = decode_adc(2 | adc_ctrl::MANUAL, ta | tb << 16 | 1 << 31, 7 | 9 << 8, 12);
        assert!(l.manual && !l.echo && !l.locked);
        assert_eq!(l.source, 2);
        assert_eq!((l.groups[0].phase, l.groups[0].locked, l.groups[0].eye, l.groups[0].delay), (2, true, 3, 7));
        assert_eq!((l.groups[1].phase, l.groups[1].locked, l.groups[1].delay), (4, false, 9));
        // changes at phases 0 and 4: a sample at 4 sits between two changes (4 and 0)
        assert_eq!(l.groups[0].clean, 0b01111);
        assert_eq!(adc_phases(2, 3), 2 << 5 | 3 << 8);
    }

    #[test]
    fn probe_keeps_one_range() {
        let mut p = Probe::new(Some(2));
        for (c, r, d) in [(100, 2, false), (102, 2, false), (5000, 1, false), (9000, 2, true), (104, 2, false)] {
            p.sample(c, 7, r, d);
        }
        let q = p.result();
        assert_eq!((q.n, q.dirty, q.other, q.min, q.max), (3, 1, 1, 100, 104));
        assert!((q.mean - 102.0).abs() < 1e-9 && (q.sd - 2.0).abs() < 1e-9 && !q.all_dirty);
        let mut a = Probe::new(None);
        a.sample(10, 0, 0, true);
        a.sample(20, 0, 5, true);
        let q = a.result();
        assert!(q.all_dirty && q.n == 0 && q.dirty == 2 && (q.mean - 15.0).abs() < 1e-9);
    }

    #[test]
    fn reg_and_flash_responses() {
        assert_eq!(parse_reg(&[34, 0, 0, 0, 0x0B, 0, 0x42, 0x4E]), Some(0x4E42_000B));
        assert_eq!(parse_reg(&[1, 2]), None);
        let mut p = vec![0, 1, 0, 0, 3, 0, 0, 0];
        p.extend_from_slice(&[9, 8, 7]);
        assert_eq!(parse_flash_read(&p), Some(vec![9, 8, 7]));
        assert_eq!(parse_flash_read(&p[..9]), None);
        assert_eq!(reg_name(41), "CLK_HZ");
    }
}

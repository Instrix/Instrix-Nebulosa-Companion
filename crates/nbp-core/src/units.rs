//! Codes to physical units.
//!
//! Defaults are the firmware's own scaling (Instrix-Nebulosa-Firmware,
//! boards/digital/src/app/meas.c and meas_fpga.c): current = code × the
//! block's range full scale / 16384, voltage = code × 1 mV. The voltage
//! scale is the firmware's debug scaling; replace both with calibration data
//! when there is some.

use serde::{Deserialize, Serialize};

pub const CODE_FS: f64 = 16384.0;
pub const RANGES: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// full scale per current range, A (range 0 = the highest)
    pub i_fs: [f64; RANGES],
    /// offset per current range, A (added after scaling)
    pub i_offset: [f64; RANGES],
    /// V per code
    pub v_scale: f64,
    /// V
    pub v_offset: f64,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            i_fs: [2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6, 0.0, 0.0],
            i_offset: [0.0; RANGES],
            v_scale: 1e-3,
            v_offset: 0.0,
        }
    }
}

impl Calibration {
    #[inline]
    pub fn current(&self, code: u16, range: u8) -> f64 {
        let r = range as usize & (RANGES - 1);
        code as f64 * self.i_fs[r] / CODE_FS + self.i_offset[r]
    }

    #[inline]
    pub fn voltage(&self, code: u16) -> f64 {
        code as f64 * self.v_scale + self.v_offset
    }
}

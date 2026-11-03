//! NBP, the Nebulosa Bulk Protocol, host side.
//!
//! The definition is `common/nbp/nbp.h` in Instrix-Nebulosa-Firmware; the
//! constants and layouts here mirror it. Nothing in this crate does I/O:
//! transports (native USB / TCP, WebUSB through the `wasm` feature) feed it
//! bytes and send the bytes it encodes.
//!
//! - [`analysis`]: histogram / CDF, PSD, worst window over a span
//! - [`buffer`]: every sample, with a summary pyramid for fast span queries
//! - [`export`]: logger output: CSV / TXT / JSON Lines / HDF5 columns, configurable fields
//! - [`frame`]: message header, encoding, a resyncing byte-stream parser
//! - [`math`]: the math channel's expressions
//! - [`msg`]: command codes and payload structs
//! - [`stream`]: the FPGA sample stream, format 5
//! - [`units`]: codes to A / V (calibration)
//! - [`trigger`]: edge trigger on the full-rate samples
//! - [`recording`]: the .nbrec file format
//! - [`session`]: the session log: interval summaries over days, bounded memory, .nblog files
//! - [`pipeline`]: all of the above for one connection

pub mod analysis;
pub mod buffer;
pub mod export;
pub mod frame;
pub mod math;
pub mod msg;
pub mod pipeline;
pub mod recording;
pub mod session;
pub mod stream;
pub mod trigger;
pub mod units;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use frame::{encode, Frame, FrameParser};
pub use pipeline::{Accum, DataStats, Meta, Pipeline};
pub use units::Calibration;

pub(crate) fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub(crate) fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

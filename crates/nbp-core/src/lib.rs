//! NBP, the Nebulosa Bulk Protocol, host side.
//!
//! The definition is `common/nbp/nbp.h` in Instrix-Nebulosa-Firmware; the
//! constants and layouts here mirror it. Nothing in this crate does I/O:
//! transports (native USB / TCP, WebUSB through the `wasm` feature) feed it
//! bytes and send the bytes it encodes.
//!
//! - [`frame`]: message header, encoding, a resyncing byte-stream parser
//! - [`msg`]: command codes and payload structs
//! - [`stream`]: the FPGA sample stream, format 5
//! - [`units`]: codes to A / V (calibration)
//! - [`reduce`]: min / max / mean / M2 reduction of samples for display
//! - [`pipeline`]: all of the above for one connection

pub mod frame;
pub mod msg;
pub mod pipeline;
pub mod reduce;
pub mod stream;
pub mod units;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use frame::{encode, Frame, FrameParser};
pub use pipeline::{Accum, DataStats, Meta, Pipeline};
pub use units::Calibration;
pub use reduce::Batch;

pub(crate) fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub(crate) fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

//! Everything one connection needs on the receive side: a frame parser per
//! source (each USB IN endpoint is its own byte stream; TCP is one), DATA
//! block checks, the format-5 decoder, conversion to physical units,
//! charge / energy integration and the display reducer.
//!
//! Transports push bytes in and take out responses, point batches and
//! [`Meta`] (counters, accumulators).

use crate::frame::{Frame, FrameParser};
use crate::msg::{self, BlockHdr};
use crate::reduce::{Batch, Phys, Reducer};
use crate::stream::{StreamCounters, StreamDecoder, Word, TS_PERIOD_S};
use crate::units::Calibration;
use serde::Serialize;

pub const MAX_SOURCES: usize = 4;

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct DataStats {
    /// DATA messages received since the stream's FIRST block
    pub blocks: u64,
    /// sum of jumps in the DATA block sequence
    pub lost_blocks: u64,
    pub stall_flags: u64,
    pub gap_flags: u64,
    pub test_blocks: u64,
    pub bad_blocks: u64,
    pub bytes: u64,
    /// bytes discarded by the frame parsers to find a magic
    pub resyncs: u64,
    pub stream: StreamCounters,
}

/// Integrals over every sample since the stream started.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Accum {
    /// C
    pub charge: f64,
    /// J
    pub energy: f64,
    /// s of samples integrated
    pub seconds: f64,
}

/// Everything but the points; small, sent with every batch.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Meta {
    pub stats: DataStats,
    pub accum: Accum,
    /// decimation of the latest FPGA block header: N = 2^log2n
    pub log2n: u8,
    /// bumped at every stream start (FIRST block)
    pub stream_id: u32,
}

pub struct Pipeline {
    parsers: [FrameParser; MAX_SOURCES],
    decoder: StreamDecoder,
    reducer: Reducer,
    responses: Vec<Frame>,
    expect_seq: Option<u32>,
    stats: DataStats,
    cal: Calibration,
    accum: Accum,
    stream_id: u32,
    fresh: bool,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            parsers: Default::default(),
            decoder: StreamDecoder::new(),
            reducer: Reducer::default(),
            responses: Vec::new(),
            expect_seq: None,
            stats: DataStats::default(),
            cal: Calibration::default(),
            accum: Accum::default(),
            stream_id: 0,
            fresh: false,
        }
    }

    pub fn set_bucket(&mut self, n: u32) {
        self.reducer.set_bucket(n);
    }

    /// Applies to samples from now on.
    pub fn set_calibration(&mut self, cal: Calibration) {
        self.cal = cal;
    }

    pub fn calibration(&self) -> &Calibration {
        &self.cal
    }

    /// Bytes from source `src` (an IN endpoint index; 0 for TCP).
    pub fn push(&mut self, src: usize, bytes: &[u8]) {
        let src = src.min(MAX_SOURCES - 1);
        self.stats.bytes += bytes.len() as u64;
        self.parsers[src].push(bytes);
        while let Some(f) = self.parsers[src].next_frame() {
            if f.ty == msg::DATA {
                self.on_data(&f.payload);
            } else {
                self.responses.push(f);
            }
        }
        self.stats.resyncs = self.parsers.iter().map(|p| p.resyncs).sum();
    }

    fn on_data(&mut self, p: &[u8]) {
        let Some(h) = BlockHdr::parse(p) else {
            self.stats.bad_blocks += 1;
            return;
        };
        if h.flags & msg::BLK_FIRST != 0 {
            let resyncs = self.stats.resyncs;
            self.decoder = StreamDecoder::new();
            self.reducer.reset();
            self.stats = DataStats { resyncs, ..Default::default() };
            self.expect_seq = None;
            self.accum = Accum::default();
            self.stream_id = self.stream_id.wrapping_add(1);
            self.fresh = true;
        }
        self.stats.blocks += 1;
        if let Some(e) = self.expect_seq {
            if h.seq != e {
                self.stats.lost_blocks += h.seq.wrapping_sub(e) as u64;
                self.decoder.desync();
            }
        }
        self.expect_seq = Some(h.seq.wrapping_add(1));
        if h.flags & msg::BLK_TEST != 0 {
            self.stats.test_blocks += 1;
            return;
        }
        if h.flags & msg::BLK_STALL != 0 {
            self.stats.stall_flags += 1;
            self.decoder.desync();
        }
        if h.flags & msg::BLK_GAP != 0 {
            self.stats.gap_flags += 1;
            self.decoder.desync();
        }
        let n = (h.nwords as usize).min((p.len() - BlockHdr::LEN) / 4);
        for c in p[BlockHdr::LEN..BlockHdr::LEN + n * 4].chunks_exact(4) {
            let w = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
            if let Word::Sample(s) = self.decoder.feed(w) {
                let i = self.cal.current(s.current, s.range);
                let v = self.cal.voltage(s.voltage);
                let dt = s.step as f64 * TS_PERIOD_S;
                self.accum.charge += i * dt;
                self.accum.energy += i * v * dt;
                self.accum.seconds += dt;
                self.reducer.push(&Phys { ts: s.ts, step: s.step, i, v, range: s.range, dirty: s.dirty });
            }
        }
        self.stats.stream = self.decoder.c;
    }

    pub fn take_responses(&mut self) -> Vec<Frame> {
        std::mem::take(&mut self.responses)
    }

    /// New points, or `None` when there are none and nothing changed. The
    /// first batch after a FIRST block comes even if empty, so a UI can clear.
    pub fn take_batch(&mut self) -> Option<Batch> {
        let points = self.reducer.take();
        if points.is_empty() && !self.fresh {
            return None;
        }
        self.fresh = false;
        Some(points)
    }

    pub fn meta(&self) -> Meta {
        Meta { stats: self.stats, accum: self.accum, log2n: self.decoder.log2n, stream_id: self.stream_id }
    }

    pub fn stats(&self) -> DataStats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::encode;
    use crate::stream::tests::block_words;

    fn data_msg(seq: u32, flags: u16, words: &[u32]) -> Vec<u8> {
        let mut p = vec![];
        p.extend_from_slice(&seq.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&(words.len() as u16).to_le_bytes());
        p.extend_from_slice(&flags.to_le_bytes());
        for w in words {
            p.extend_from_slice(&w.to_be_bytes());
        }
        p.resize(2040, 0);
        // encode() caps command payloads; build the DATA header by hand
        let mut m = encode(msg::DATA, seq as u16, &[]);
        m[6..8].copy_from_slice(&2040u16.to_le_bytes());
        m.extend_from_slice(&p);
        m
    }

    #[test]
    fn data_and_responses() {
        let mut pl = Pipeline::new();
        pl.set_bucket(1);
        let w = block_words(0, 2, 0, 100, &[(1, 2, false), (3, 4, false)]);
        let mut bytes = data_msg(0, msg::BLK_FIRST, &w);
        bytes.extend(encode(msg::rsp(msg::cmd::PING), 5, b"x"));
        for c in bytes.chunks(512) {
            pl.push(0, c);
        }
        let r = pl.take_responses();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].seq, 5);
        let b = pl.take_batch().unwrap();
        assert_eq!(b.len(), 2);
        // code 1 in range 2 (20 mA full scale), 2 mV
        assert!((b.i.avg[0] - 20e-3 / 16384.0).abs() < 1e-15);
        assert!((b.v.avg[0] - 2e-3).abs() < 1e-15);
        let m = pl.meta();
        assert_eq!(m.stats.blocks, 1);
        assert_eq!(m.stats.stream.samples, 2);
        assert_eq!(m.stream_id, 1);
        // charge: (1 + 3) codes x 20 mA / 16384 x 500 ns
        assert!((m.accum.charge - 4.0 * 20e-3 / 16384.0 * 500e-9).abs() < 1e-20);
        // a lost DATA block is counted
        pl.push(0, &data_msg(2, 0, &[]));
        assert_eq!(pl.stats().lost_blocks, 1);
    }
}

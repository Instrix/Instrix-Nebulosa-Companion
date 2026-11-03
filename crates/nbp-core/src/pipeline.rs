//! Everything one connection needs on the receive side: a frame parser per
//! source (each USB IN endpoint is its own byte stream; TCP is one), DATA
//! block checks, the format-5 decoder, the full-rate sample buffer, the
//! since-start statistics and the charge / energy integrals.
//!
//! Transports push bytes in and take out responses and [`Meta`] (counters,
//! accumulators, what the buffer holds); the UI queries the buffer
//! ([`Pipeline::lod`], [`Pipeline::stats`], [`Pipeline::value_at`]).

use crate::frame::{Frame, FrameParser};
use crate::msg::{self, BlockHdr};
use crate::buffer::{pack, unpack, Moments3, SampleBuffer, SpanStats};
use crate::recording::{Recorder, Recording, RecordingStatus};
use crate::stream::{StreamCounters, StreamDecoder, Word, TS_PERIOD_S};
use crate::trigger::{Event, Trigger, TriggerCfg, TriggerInfo};
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

/// Everything but the samples; small, sent to the UI a few dozen times a second.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Meta {
    pub stats: DataStats,
    pub accum: Accum,
    /// every sample since the stream started
    pub run: Moments3,
    /// decimation of the latest FPGA block header: N = 2^log2n
    pub log2n: u8,
    /// bumped at every stream start (FIRST block)
    pub stream_id: u32,
    /// the buffer: whether it holds samples, their time span (s), its version
    pub has_data: bool,
    pub t_start: f64,
    pub t_end: f64,
    pub version: u64,
    pub buffer_samples: u64,
    /// s per sample at the current decimation
    pub sample_period: f64,
    pub trigger: TriggerInfo,
    pub recording: RecordingStatus,
}

/// Samples the buffer holds by default (4 bytes each): 64 MB.
pub const DEFAULT_BUFFER_SAMPLES: usize = 16 << 20;

pub struct Pipeline {
    parsers: [FrameParser; MAX_SOURCES],
    decoder: StreamDecoder,
    buf: SampleBuffer,
    run: Moments3,
    trig: Trigger,
    /// timestamp the next sample should have (a jump: samples missing)
    next_ts: Option<u64>,
    rec: Option<Recorder>,
    /// opened from a file: its header
    file_header: Option<String>,
    responses: Vec<Frame>,
    expect_seq: Option<u32>,
    stats: DataStats,
    cal: Calibration,
    accum: Accum,
    stream_id: u32,
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
            buf: SampleBuffer::new(DEFAULT_BUFFER_SAMPLES, Calibration::default()),
            run: Moments3::default(),
            trig: Trigger::default(),
            next_ts: None,
            rec: None,
            file_header: None,
            responses: Vec::new(),
            expect_seq: None,
            stats: DataStats::default(),
            cal: Calibration::default(),
            accum: Accum::default(),
            stream_id: 0,
        }
    }

    /// Resizes the sample buffer (drops what it holds).
    pub fn set_buffer_samples(&mut self, n: usize) {
        if n as u64 != self.buf.capacity() {
            self.buf = SampleBuffer::new(n, self.cal.clone());
        }
    }

    /// Rescales the buffer's samples too; the since-start statistics and the
    /// accumulators apply it from now on.
    pub fn set_trigger(&mut self, cfg: TriggerCfg) {
        self.trig.set(cfg);
    }

    /// Trigger events with seq ≥ `since` (at most `max`), oldest first.
    pub fn events_since(&self, since: u64, max: usize) -> Vec<Event> {
        self.trig.events_since(since, max)
    }

    pub fn clear_events(&mut self) {
        self.trig.clear_events();
    }

    pub fn set_calibration(&mut self, cal: Calibration) {
        if let Some(r) = &mut self.rec {
            r.calibration(&cal);
        }
        self.buf.set_calibration(cal.clone());
        self.cal = cal;
    }

    pub fn segments(&self) -> usize {
        self.buf.segments()
    }

    pub fn lod(&self, t0: f64, t1: f64, bins: usize) -> Vec<f64> {
        self.buf.lod(t0, t1, bins)
    }

    pub fn stats(&self, t0: f64, t1: f64) -> SpanStats {
        self.buf.stats(t0, t1)
    }

    /// (A, V, range) of the sample at or just before t
    pub fn value_at(&self, t: f64) -> Option<(f64, f64, u8)> {
        self.buf.value_at(t)
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

    /// One sample, decoded: buffer, statistics, integrals, trigger, recording.
    #[inline]
    fn sample(&mut self, ts: u64, step: u32, ic: u16, vc: u16, range: u8, dirty: bool) {
        let i = self.cal.current(ic, range);
        let v = self.cal.voltage(vc);
        let dt = step as f64 * TS_PERIOD_S;
        self.accum.charge += i * dt;
        self.accum.energy += i * v * dt;
        self.accum.seconds += dt;
        self.run.i.add(i);
        self.run.v.add(v);
        self.run.p.add(i * v);
        self.buf.push(ts, step, ic, vc, range, dirty);
        if self.trig.enabled() {
            if self.next_ts != Some(ts) {
                self.trig.gap();
            }
            self.trig.feed(ts as f64 * TS_PERIOD_S, self.trig.value(i, v), dt);
        }
        if let Some(r) = &mut self.rec {
            r.sample(ts, step, pack(ic, vc, range, dirty));
        }
        self.next_ts = Some(ts + step as u64);
    }

    // ---- recording ----------------------------------------------------------

    /// Starts writing every sample from now on; the file's bytes come from
    /// [`Pipeline::take_recording`].
    pub fn start_recording(&mut self, header_json: &str) {
        self.rec = Some(Recorder::new(header_json, &self.cal));
    }

    /// Bytes for the file since the last call (whole blocks unless `flush`).
    pub fn take_recording(&mut self, flush: bool) -> Vec<u8> {
        self.rec.as_mut().map(|r| r.take(flush)).unwrap_or_default()
    }

    /// The file's last bytes; recording stops.
    pub fn stop_recording(&mut self) -> Option<(Vec<u8>, RecordingStatus)> {
        let r = self.rec.take()?;
        let mut st = r.status;
        let last = r.finish();
        st.bytes += last.len() as u64;
        st.active = false;
        Some((last, st))
    }

    /// A recording, loaded whole (its samples, statistics, integrals).
    pub fn from_recording(bytes: &[u8]) -> Result<Pipeline, String> {
        let rec = Recording::parse(bytes)?;
        let mut p = Pipeline::new();
        p.buf = SampleBuffer::new(rec.samples().max(1) as usize, rec.calibration.clone());
        p.cal = rec.calibration.clone();
        p.stream_id = 1;
        for (ts0, step, s) in &rec.blocks {
            for (k, &w) in s.iter().enumerate() {
                let (ic, vc, r, d) = unpack(w);
                p.sample(ts0 + k as u64 * *step as u64, *step, ic, vc, r, d);
            }
            p.decoder.log2n = step.trailing_zeros() as u8;
        }
        p.buf.touch();
        p.file_header = Some(rec.header_json);
        Ok(p)
    }

    pub fn file_header(&self) -> Option<&str> {
        self.file_header.as_deref()
    }

    /// Every sample in [t0, t1): f(time s, A, V, range)
    pub fn for_each_sample(&self, t0: f64, t1: f64, f: impl FnMut(f64, f64, f64, u8)) {
        self.buf.for_each(t0, t1, f);
    }

    pub fn count_samples(&self, t0: f64, t1: f64) -> u64 {
        self.buf.count(t0, t1)
    }

    fn on_data(&mut self, p: &[u8]) {
        let Some(h) = BlockHdr::parse(p) else {
            self.stats.bad_blocks += 1;
            return;
        };
        if h.flags & msg::BLK_FIRST != 0 {
            let resyncs = self.stats.resyncs;
            self.decoder = StreamDecoder::new();
            self.buf.clear();
            self.run = Moments3::default();
            self.trig.reset();
            self.next_ts = None;
            self.stats = DataStats { resyncs, ..Default::default() };
            self.expect_seq = None;
            self.accum = Accum::default();
            self.stream_id = self.stream_id.wrapping_add(1);
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
                self.sample(s.ts, s.step, s.current, s.voltage, s.range, s.dirty);
            }
        }
        self.buf.touch();
        self.stats.stream = self.decoder.c;
    }

    pub fn take_responses(&mut self) -> Vec<Frame> {
        std::mem::take(&mut self.responses)
    }

    pub fn meta(&self) -> Meta {
        let range = self.buf.time_range();
        Meta {
            stats: self.stats,
            accum: self.accum,
            run: self.run,
            log2n: self.decoder.log2n,
            stream_id: self.stream_id,
            has_data: range.is_some(),
            t_start: range.map_or(0.0, |r| r.0),
            t_end: range.map_or(0.0, |r| r.1),
            version: self.buf.version,
            buffer_samples: self.buf.capacity(),
            sample_period: (1u64 << self.decoder.log2n) as f64 * TS_PERIOD_S,
            trigger: self.trig.info(),
            recording: self.rec.as_ref().map(|r| r.status).unwrap_or_default(),
        }
    }

    pub fn data_stats(&self) -> DataStats {
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
        pl.set_buffer_samples(1 << 16);
        let w = block_words(0, 2, 0, 100, &[(1, 2, false), (3, 4, false)]);
        let mut bytes = data_msg(0, msg::BLK_FIRST, &w);
        bytes.extend(encode(msg::rsp(msg::cmd::PING), 5, b"x"));
        for c in bytes.chunks(512) {
            pl.push(0, c);
        }
        let r = pl.take_responses();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].seq, 5);
        // code 1 in range 2 (20 mA full scale), 2 mV, at timestamp 100
        let (i, v, r) = pl.value_at(100.0 * 500e-9).unwrap();
        assert!((i - 20e-3 / 16384.0).abs() < 1e-15);
        assert!((v - 2e-3).abs() < 1e-15);
        assert_eq!(r, 2);
        let s = pl.stats(0.0, 1.0);
        assert_eq!(s.m.n(), 2);
        assert!((s.m.i.mean - 2.0 * 20e-3 / 16384.0).abs() < 1e-15);
        let m = pl.meta();
        assert_eq!(m.stats.blocks, 1);
        assert_eq!(m.stats.stream.samples, 2);
        assert_eq!(m.stream_id, 1);
        assert!(m.has_data);
        assert_eq!(m.run.n(), 2);
        // charge: (1 + 3) codes x 20 mA / 16384 x 500 ns
        assert!((m.accum.charge - 4.0 * 20e-3 / 16384.0 * 500e-9).abs() < 1e-20);
        // a lost DATA block is counted
        pl.push(0, &data_msg(2, 0, &[]));
        assert_eq!(pl.data_stats().lost_blocks, 1);
    }
}

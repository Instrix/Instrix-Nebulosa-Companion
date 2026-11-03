//! Recording file format (.nbrec): every sample, as the buffer keeps them.
//!
//! ```text
//! magic    8 bytes  "NBREC\0" 0x01 0x00 (format 1)
//! records  tag u8, payload length u32 LE, payload:
//!   'H'  header: JSON, written by the app (device info, settings, time)
//!   'C'  calibration: JSON (units.rs Calibration), at the start and when it changes
//!   'S'  samples: ts0 u64, step u32, count u32, then count × u32 packed samples
//!        (buffer.rs layout); evenly spaced: sample k at ts0 + k × step
//!        (500 ns periods)
//!   'E'  end: JSON {samples, seconds}
//! ```
//! A file cut short (a crash, a full disk) reads up to its last whole record.
//!
//! Encoding and decoding only; the app does the file I/O.

use crate::units::Calibration;
use serde::Serialize;

pub const MAGIC: &[u8; 8] = b"NBREC\0\x01\0";
const BLOCK_SAMPLES: usize = 1 << 16;

fn record(tag: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct RecordingStatus {
    pub active: bool,
    pub samples: u64,
    pub bytes: u64,
    /// s of samples
    pub seconds: f64,
}

/// Turns samples into records; the app takes the bytes and writes them.
pub struct Recorder {
    out: Vec<u8>,
    ts0: u64,
    step: u32,
    block: Vec<u32>,
    pub status: RecordingStatus,
}

impl Recorder {
    /// The file's first bytes are in the first `take()`.
    pub fn new(header_json: &str, cal: &Calibration) -> Self {
        let mut out = MAGIC.to_vec();
        record(b'H', header_json.as_bytes(), &mut out);
        let mut r = Self {
            out,
            ts0: 0,
            step: 1,
            block: Vec::with_capacity(BLOCK_SAMPLES),
            status: RecordingStatus { active: true, ..Default::default() },
        };
        r.calibration(cal);
        r
    }

    pub fn calibration(&mut self, cal: &Calibration) {
        self.flush();
        // serde_json can't fail on this struct
        let j = serde_json::to_vec(cal).unwrap_or_default();
        record(b'C', &j, &mut self.out);
    }

    #[inline]
    pub fn sample(&mut self, ts: u64, step: u32, packed: u32) {
        let n = self.block.len() as u64;
        if n > 0 && (step != self.step || self.ts0 + n * self.step as u64 != ts) {
            self.flush();
        }
        if self.block.is_empty() {
            self.ts0 = ts;
            self.step = step;
        }
        self.block.push(packed);
        self.status.samples += 1;
        self.status.seconds += step as f64 * crate::stream::TS_PERIOD_S;
        if self.block.len() == BLOCK_SAMPLES {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.block.is_empty() {
            return;
        }
        let mut p = Vec::with_capacity(16 + self.block.len() * 4);
        p.extend_from_slice(&self.ts0.to_le_bytes());
        p.extend_from_slice(&self.step.to_le_bytes());
        p.extend_from_slice(&(self.block.len() as u32).to_le_bytes());
        for w in &self.block {
            p.extend_from_slice(&w.to_le_bytes());
        }
        record(b'S', &p, &mut self.out);
        self.block.clear();
    }

    /// Bytes ready for the file. Whole sample blocks only (64 Ki samples,
    /// ~33 ms at 2 MS/s), unless `flush`.
    pub fn take(&mut self, flush: bool) -> Vec<u8> {
        if flush {
            self.flush();
        }
        let b = std::mem::take(&mut self.out);
        self.status.bytes += b.len() as u64;
        b
    }

    /// The last bytes, end record included.
    pub fn finish(mut self) -> Vec<u8> {
        self.flush();
        let end = format!("{{\"samples\":{},\"seconds\":{}}}", self.status.samples, self.status.seconds);
        record(b'E', end.as_bytes(), &mut self.out);
        self.take(false)
    }
}

/// What a recording holds, read back.
pub struct Recording {
    pub header_json: String,
    pub calibration: Calibration,
    /// (ts0, step, packed samples)
    pub blocks: Vec<(u64, u32, Vec<u32>)>,
    /// the file ended without an end record (cut short)
    pub truncated: bool,
}

impl Recording {
    pub fn samples(&self) -> u64 {
        self.blocks.iter().map(|b| b.2.len() as u64).sum()
    }

    pub fn parse(b: &[u8]) -> Result<Recording, String> {
        if b.len() < 8 || &b[..8] != MAGIC {
            return Err("not a Nebulosa recording (.nbrec)".into());
        }
        let mut rec = Recording { header_json: "{}".into(), calibration: Calibration::default(), blocks: Vec::new(), truncated: true };
        let mut p = 8;
        while p + 5 <= b.len() {
            let tag = b[p];
            let len = u32::from_le_bytes([b[p + 1], b[p + 2], b[p + 3], b[p + 4]]) as usize;
            if p + 5 + len > b.len() {
                break; // cut short
            }
            let d = &b[p + 5..p + 5 + len];
            p += 5 + len;
            match tag {
                b'H' => rec.header_json = String::from_utf8_lossy(d).into_owned(),
                b'C' => {
                    if let Ok(c) = serde_json::from_slice(d) {
                        // samples are kept as codes: the last calibration applies to all
                        rec.calibration = c;
                    }
                }
                b'S' if d.len() >= 16 => {
                    let ts0 = u64::from_le_bytes(d[0..8].try_into().unwrap());
                    let step = u32::from_le_bytes(d[8..12].try_into().unwrap());
                    let n = (u32::from_le_bytes(d[12..16].try_into().unwrap()) as usize).min((d.len() - 16) / 4);
                    let s = d[16..16 + n * 4].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
                    rec.blocks.push((ts0, step, s));
                }
                b'E' => rec.truncated = false,
                _ => {} // unknown records: newer writers may add some
            }
        }
        Ok(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_gap_and_truncation() {
        let mut r = Recorder::new("{\"x\":1}", &Calibration::default());
        let mut file = r.take(false);
        for k in 0..100_000u64 {
            r.sample(k, 1, k as u32);
        }
        for k in 0..10u64 {
            r.sample(1_000_000 + k * 4, 4, 7);
        }
        file.extend(r.take(false));
        file.extend(r.finish());
        let rec = Recording::parse(&file).unwrap();
        assert_eq!(rec.header_json, "{\"x\":1}");
        assert!(!rec.truncated);
        assert_eq!(rec.samples(), 100_010);
        assert_eq!(rec.blocks.len(), 3); // 65536 + 34464, then the gap
        assert_eq!(rec.blocks[2].0, 1_000_000);
        assert_eq!(rec.blocks[2].1, 4);
        assert_eq!(rec.blocks[1].2[0], 65536);
        // cut short in the middle of the last block: the whole ones still read
        // (past the end record, ~40 bytes, into the last 10-sample block)
        let cut = Recording::parse(&file[..file.len() - 60]).unwrap();
        assert!(cut.truncated);
        assert_eq!(cut.samples(), 100_000);
    }
}

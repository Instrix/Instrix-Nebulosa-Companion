//! Display reduction: `bucket` consecutive samples become one point with,
//! for current, voltage and power each, min / max / mean and M2 (the sum of
//! squared deviations from the mean), plus the sample count. Count, mean and
//! M2 let a consumer merge points into exact statistics over any span (Chan
//! et al.'s parallel variance).
//!
//! A bucket also ends at a range change and at a timestamp gap; a gap also
//! emits a gap point (n = 0, values NaN) so a plot breaks the line there.

use crate::stream::TS_PERIOD_S;
use serde::Serialize;

/// One sample in physical units.
#[derive(Debug, Clone, Copy)]
pub struct Phys {
    pub ts: u64,
    pub step: u32,
    /// A
    pub i: f64,
    /// V
    pub v: f64,
    pub range: u8,
    pub dirty: bool,
}

/// Running min / max / mean / M2 of one channel (Welford).
#[derive(Debug, Clone, Copy)]
pub struct Welford {
    pub n: u32,
    pub mean: f64,
    pub m2: f64,
    pub min: f64,
    pub max: f64,
}

impl Default for Welford {
    fn default() -> Self {
        Self { n: 0, mean: 0.0, m2: 0.0, min: f64::INFINITY, max: f64::NEG_INFINITY }
    }
}

impl Welford {
    #[inline]
    pub fn add(&mut self, x: f64) {
        self.n += 1;
        let d = x - self.mean;
        self.mean += d / self.n as f64;
        self.m2 += d * (x - self.mean);
        if x < self.min {
            self.min = x;
        }
        if x > self.max {
            self.max = x;
        }
    }
}

/// Columns of one channel.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ChanCols {
    pub min: Vec<f64>,
    pub max: Vec<f64>,
    pub avg: Vec<f64>,
    pub m2: Vec<f64>,
}

impl ChanCols {
    fn push(&mut self, w: &Welford) {
        self.min.push(w.min);
        self.max.push(w.max);
        self.avg.push(w.mean);
        self.m2.push(w.m2);
    }

    fn push_gap(&mut self) {
        self.min.push(f64::NAN);
        self.max.push(f64::NAN);
        self.avg.push(f64::NAN);
        self.m2.push(0.0);
    }
}

/// Points, as columns.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Batch {
    /// s since FPGA reset, of the point's first sample
    pub t: Vec<f64>,
    /// samples in the point; 0 = gap point
    pub n: Vec<u32>,
    pub range: Vec<u8>,
    /// a dirty sample went into the point
    pub dirty: Vec<bool>,
    pub i: ChanCols,
    pub v: ChanCols,
    pub p: ChanCols,
}

/// Version of [`Batch::to_f64`]'s layout.
pub const BATCH_LAYOUT: f64 = 1.0;
pub const BATCH_COLS: usize = 16;

impl Batch {
    pub fn len(&self) -> usize {
        self.t.len()
    }

    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    /// Flat transport form, the same for the native IPC and WASM:
    /// `[layout, points, columns, then each column]`; columns in order t, n,
    /// range, dirty, then i, v, p each min, max, avg, m2.
    pub fn to_f64(&self) -> Vec<f64> {
        let n = self.len();
        let mut v = Vec::with_capacity(3 + n * BATCH_COLS);
        v.extend_from_slice(&[BATCH_LAYOUT, n as f64, BATCH_COLS as f64]);
        v.extend_from_slice(&self.t);
        v.extend(self.n.iter().map(|&x| x as f64));
        v.extend(self.range.iter().map(|&x| x as f64));
        v.extend(self.dirty.iter().map(|&x| x as u8 as f64));
        for c in [&self.i, &self.v, &self.p] {
            v.extend_from_slice(&c.min);
            v.extend_from_slice(&c.max);
            v.extend_from_slice(&c.avg);
            v.extend_from_slice(&c.m2);
        }
        v
    }

    fn gap(&mut self, ts: u64, range: u8) {
        self.t.push(ts as f64 * TS_PERIOD_S);
        self.n.push(0);
        self.range.push(range);
        self.dirty.push(false);
        self.i.push_gap();
        self.v.push_gap();
        self.p.push_gap();
    }
}

#[derive(Default)]
struct Acc {
    n: u32,
    ts0: u64,
    range: u8,
    dirty: bool,
    i: Welford,
    v: Welford,
    p: Welford,
}

pub struct Reducer {
    bucket: u32,
    acc: Acc,
    /// where the last sample left off, for gap detection across buckets
    last_next_ts: Option<u64>,
    out: Batch,
}

impl Default for Reducer {
    fn default() -> Self {
        Self::new(64)
    }
}

impl Reducer {
    pub fn new(bucket: u32) -> Self {
        Self { bucket: bucket.max(1), acc: Acc::default(), last_next_ts: None, out: Batch::default() }
    }

    pub fn set_bucket(&mut self, n: u32) {
        self.flush();
        self.bucket = n.max(1);
    }

    pub fn reset(&mut self) {
        self.acc = Acc::default();
        self.last_next_ts = None;
        self.out = Batch::default();
    }

    pub fn push(&mut self, s: &Phys) {
        if let Some(next) = self.last_next_ts {
            if s.ts != next {
                self.flush();
                self.out.gap(next, s.range);
            }
        }
        if self.acc.n > 0 && s.range != self.acc.range {
            self.flush();
        }
        let a = &mut self.acc;
        if a.n == 0 {
            *a = Acc { ts0: s.ts, range: s.range, ..Acc::default() };
        }
        a.n += 1;
        a.dirty |= s.dirty;
        a.i.add(s.i);
        a.v.add(s.v);
        a.p.add(s.i * s.v);
        self.last_next_ts = Some(s.ts + s.step as u64);
        if a.n >= self.bucket {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        let a = &self.acc;
        if a.n == 0 {
            return;
        }
        let o = &mut self.out;
        o.t.push(a.ts0 as f64 * TS_PERIOD_S);
        o.n.push(a.n);
        o.range.push(a.range);
        o.dirty.push(a.dirty);
        o.i.push(&a.i);
        o.v.push(&a.v);
        o.p.push(&a.p);
        self.acc.n = 0;
    }

    /// Points so far. A partial bucket stays until it fills, so points keep
    /// one bucket's width.
    pub fn take(&mut self) -> Batch {
        std::mem::take(&mut self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(ts: u64, i: f64, range: u8) -> Phys {
        Phys { ts, step: 1, i, v: 2.0, range, dirty: false }
    }

    #[test]
    fn buckets_ranges_gaps() {
        let mut r = Reducer::new(2);
        r.push(&s(0, 1.0, 0));
        r.push(&s(1, 5.0, 0)); // bucket full
        r.push(&s(2, 3.0, 1)); // new range
        r.push(&s(10, 4.0, 1)); // gap: flush, gap point, new bucket
        r.flush();
        let b = r.take();
        assert_eq!(b.len(), 4);
        assert_eq!((b.i.min[0], b.i.max[0]), (1.0, 5.0));
        assert_eq!((b.n[0], b.i.avg[0], b.i.m2[0]), (2, 3.0, 8.0)); // (1-3)^2 + (5-3)^2
        assert_eq!((b.p.avg[0], b.p.m2[0]), (6.0, 32.0)); // P = 2 I
        assert_eq!(b.range[1], 1);
        assert_eq!(b.n[2], 0);
        assert!(b.i.avg[2].is_nan());
        assert_eq!(b.i.avg[3], 4.0);
        let f = b.to_f64();
        assert_eq!(f.len(), 3 + 4 * BATCH_COLS);
        assert_eq!(f[3 + 4], 2.0); // n column, first point
    }
}

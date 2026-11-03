//! Full-rate sample buffer with a summary pyramid.
//!
//! Every sample is kept, packed in 4 bytes (current code, voltage code,
//! range, dirty), in a ring of fixed capacity. Above it, summary levels of
//! 64, 4096 and 262144 samples per entry hold count, mean, M2 (sum of squared
//! deviations), min and max for current, voltage and power. Any span reduces
//! with the coarsest entries that fit in it and raw samples only at its edges,
//! so a query costs about the same for 20 samples or the whole buffer.
//!
//! Time: samples come in segments of evenly spaced FPGA timestamps (500 ns
//! periods × the decimation step). A new segment starts at a gap or a step
//! change; queries map time to sample indices within each segment.
//!
//! Calibration is applied when samples are read; changing it rebuilds the
//! summaries from the raw samples.

use crate::stream::TS_PERIOD_S;
use crate::units::Calibration;
use serde::Serialize;
use std::collections::VecDeque;

const LEVEL_SHIFTS: [u32; 3] = [6, 12, 18];

/// Packed sample: [13:0] current code, [27:14] voltage code, [30:28] range, [31] dirty.
#[inline]
pub fn pack(i: u16, v: u16, range: u8, dirty: bool) -> u32 {
    (i as u32 & 0x3FFF) | ((v as u32 & 0x3FFF) << 14) | ((range as u32 & 7) << 28) | ((dirty as u32) << 31)
}

#[inline]
pub fn unpack(w: u32) -> (u16, u16, u8, bool) {
    ((w & 0x3FFF) as u16, ((w >> 14) & 0x3FFF) as u16, ((w >> 28) & 7) as u8, w >> 31 != 0)
}

/// count / mean / M2 / min / max of one channel (Chan et al. merge).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Moments {
    pub n: u64,
    pub mean: f64,
    pub m2: f64,
    pub min: f64,
    pub max: f64,
}

impl Default for Moments {
    fn default() -> Self {
        Self { n: 0, mean: 0.0, m2: 0.0, min: f64::INFINITY, max: f64::NEG_INFINITY }
    }
}

impl Moments {
    #[inline]
    pub fn add(&mut self, x: f64) {
        self.n += 1;
        let d = x - self.mean;
        self.mean += d / self.n as f64;
        self.m2 += d * (x - self.mean);
        self.min = self.min.min(x);
        self.max = self.max.max(x);
    }

    #[inline]
    pub fn merge(&mut self, o: &Moments) {
        if o.n == 0 {
            return;
        }
        let n = self.n + o.n;
        let d = o.mean - self.mean;
        self.mean += d * o.n as f64 / n as f64;
        self.m2 += o.m2 + d * d * self.n as f64 * o.n as f64 / n as f64;
        self.n = n;
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
    }

    pub fn std(&self) -> f64 {
        if self.n > 1 {
            (self.m2 / self.n as f64).sqrt()
        } else {
            0.0
        }
    }
}

/// Current, voltage, power, and the math channel (empty without one; its
/// non-finite values, like V/I at I = 0, are left out).
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Moments3 {
    pub i: Moments,
    pub v: Moments,
    pub p: Moments,
    pub m: Moments,
}

impl Moments3 {
    #[inline]
    pub(crate) fn add(&mut self, i: f64, v: f64, m: f64) {
        self.i.add(i);
        self.v.add(v);
        self.p.add(i * v);
        if m.is_finite() {
            self.m.add(m);
        }
    }

    #[inline]
    fn merge(&mut self, o: &Moments3) {
        self.i.merge(&o.i);
        self.v.merge(&o.v);
        self.p.merge(&o.p);
        self.m.merge(&o.m);
    }

    pub fn n(&self) -> u64 {
        self.i.n
    }
}

/// Raw sums for building a level-0 entry fast (no division per sample).
#[derive(Default, Clone, Copy)]
struct Sums {
    /// per channel: the math channel's count can be lower (non-finite values)
    n: [u32; 4],
    s: [f64; 4],
    q: [f64; 4],
    lo: [f64; 4],
    hi: [f64; 4],
}

impl Sums {
    fn reset(&mut self) {
        *self = Sums { lo: [f64::INFINITY; 4], hi: [f64::NEG_INFINITY; 4], ..Default::default() };
    }

    #[inline]
    fn add(&mut self, i: f64, v: f64, m: f64) {
        let x = [i, v, i * v, m];
        let k_end = if m.is_finite() { 4 } else { 3 };
        for k in 0..k_end {
            self.n[k] += 1;
            self.s[k] += x[k];
            self.q[k] += x[k] * x[k];
            self.lo[k] = self.lo[k].min(x[k]);
            self.hi[k] = self.hi[k].max(x[k]);
        }
    }

    fn moments(&self) -> Moments3 {
        let m = |k: usize| {
            if self.n[k] == 0 {
                return Moments::default();
            }
            let n = self.n[k] as f64;
            let mean = self.s[k] / n;
            Moments { n: self.n[k] as u64, mean, m2: (self.q[k] - self.s[k] * mean).max(0.0), min: self.lo[k], max: self.hi[k] }
        };
        Moments3 { i: m(0), v: m(1), p: m(2), m: m(3) }
    }
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    /// first sample index
    start: u64,
    /// its FPGA timestamp, 500 ns periods
    ts0: u64,
    /// timestamp step per sample
    step: u32,
}

struct Level {
    shift: u32,
    ring: Vec<Moments3>,
    /// entry being built (level 0 from raw sums, above from the level below)
    pending: Moments3,
}

/// What a query found over a span.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SpanStats {
    pub m: Moments3,
    /// time of the first and last sample, s (NaN if none)
    pub t0: f64,
    pub t1: f64,
    /// ∫ I dt, C
    pub charge: f64,
    /// ∫ P dt, J
    pub energy: f64,
}

/// Version of [`SampleBuffer::lod`]'s flat layout.
pub const LOD_LAYOUT: f64 = 3.0;
pub const LOD_COLS: usize = 18;

pub struct SampleBuffer {
    cap: u64,
    raw: Vec<u32>,
    /// samples ever written: the next index
    head: u64,
    segs: VecDeque<Segment>,
    levels: Vec<Level>,
    sums: Sums,
    cal: Calibration,
    /// the math channel: its program, and its value for every sample (f32)
    math: Option<crate::math::MathProgram>,
    mraw: Vec<f32>,
    /// the level-0 entry being built: its first sample's time and the time it covers
    l0: (f64, f64),
    /// level-0 entries completed since the last take (for the session log)
    completed: Vec<(f64, f64, Moments3)>,
    rebuilding: bool,
    /// bumped on every change
    pub version: u64,
}

impl SampleBuffer {
    /// `cap` samples (4 bytes each, plus about 2 % for the summaries).
    pub fn new(cap: usize, cal: Calibration) -> Self {
        let cap = cap.max(1 << 12) as u64;
        let levels = LEVEL_SHIFTS
            .iter()
            .map(|&shift| Level { shift, ring: vec![Moments3::default(); ((cap >> shift) + 2) as usize], pending: Moments3::default() })
            .collect();
        let mut s = Sums::default();
        s.reset();
        Self {
            cap,
            raw: vec![0; cap as usize],
            head: 0,
            segs: VecDeque::new(),
            levels,
            sums: s,
            cal,
            math: None,
            mraw: Vec::new(),
            l0: (0.0, 0.0),
            completed: Vec::new(),
            rebuilding: false,
            version: 0,
        }
    }

    /// Contiguous runs of samples held (gaps and decimation changes split them).
    pub fn segments(&self) -> usize {
        self.segs.len()
    }

    pub fn capacity(&self) -> u64 {
        self.cap
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.segs.clear();
        for l in &mut self.levels {
            l.pending = Moments3::default();
        }
        self.sums.reset();
        self.version += 1;
    }

    /// Oldest sample index still held.
    fn oldest(&self) -> u64 {
        self.head.saturating_sub(self.cap)
    }

    pub fn is_empty(&self) -> bool {
        self.head == 0
    }

    pub fn len(&self) -> u64 {
        self.head - self.oldest()
    }

    #[inline]
    fn phys(&self, w: u32) -> (f64, f64) {
        let (i, v, r, _) = unpack(w);
        (self.cal.current(i, r), self.cal.voltage(v))
    }

    /// The math channel's value of sample `idx` (NaN without one).
    #[inline]
    fn mval(&self, idx: u64) -> f64 {
        if self.mraw.is_empty() {
            f64::NAN
        } else {
            self.mraw[(idx % self.cap) as usize] as f64
        }
    }

    /// A math channel (or none); everything held is computed again.
    pub fn set_math(&mut self, expr: Option<&str>) -> Result<(), String> {
        match expr {
            None => {
                self.math = None;
                self.mraw = Vec::new();
            }
            Some(e) => {
                let prog = crate::math::MathProgram::compile(e)?;
                self.math = Some(prog);
                if self.mraw.len() != self.cap as usize {
                    self.mraw = vec![f32::NAN; self.cap as usize];
                }
            }
        }
        self.rebuild();
        Ok(())
    }

    pub fn has_math(&self) -> bool {
        self.math.is_some()
    }

    pub fn set_calibration(&mut self, cal: Calibration) {
        if cal == self.cal {
            return;
        }
        self.cal = cal;
        self.rebuild();
    }

    /// Summaries (and the math channel) again from the raw samples, in order
    /// (after a calibration or math change).
    fn rebuild(&mut self) {
        self.rebuilding = true;
        for l in &mut self.levels {
            l.pending = Moments3::default();
        }
        self.sums.reset();
        if let Some(m) = &mut self.math {
            m.reset_all();
        }
        for k in 0..self.segs.len() {
            let (a, b) = self.seg_bounds(k);
            let s = self.segs[k];
            if let Some(m) = &mut self.math {
                m.reset(); // a gap before each segment
            }
            for idx in a..b {
                let (i, v) = self.phys(self.raw[(idx % self.cap) as usize]);
                let mv = self.eval_math(i, v, &s, idx);
                self.summarize(idx, i, v, mv);
            }
        }
        self.rebuilding = false;
        self.version += 1;
    }

    /// Level-0 entries (64 samples) completed since the last call: (first
    /// sample's time s, time covered s, moments).
    pub fn take_completed(&mut self) -> Vec<(f64, f64, Moments3)> {
        std::mem::take(&mut self.completed)
    }

    /// The math channel for one sample, kept in its ring.
    #[inline]
    fn eval_math(&mut self, i: f64, v: f64, s: &Segment, idx: u64) -> f64 {
        let Some(m) = &mut self.math else { return f64::NAN };
        let dt = s.step as f64 * TS_PERIOD_S;
        let x = m.eval(i, v, Self::time_of(s, idx), dt);
        self.mraw[(idx % self.cap) as usize] = x as f32;
        x
    }

    /// Sample `idx` (already in the ring) into the summaries.
    #[inline]
    fn summarize(&mut self, idx: u64, i: f64, v: f64, m: f64) {
        self.sums.add(i, v, m);
        let next = idx + 1;
        if next & ((1 << LEVEL_SHIFTS[0]) - 1) != 0 {
            return;
        }
        // a level-0 entry is complete; carry it up while levels complete too
        let mut carry = self.sums.moments();
        self.sums.reset();
        if !self.rebuilding {
            self.completed.push((self.l0.0, self.l0.1, carry));
        }
        for (li, l) in self.levels.iter_mut().enumerate() {
            if li > 0 {
                l.pending.merge(&carry);
                if next & ((1 << l.shift) - 1) != 0 {
                    break;
                }
                carry = std::mem::take(&mut l.pending);
            }
            let len = l.ring.len() as u64;
            l.ring[(((next >> l.shift) - 1) % len) as usize] = carry;
        }
    }

    /// One sample at FPGA time `ts` (500 ns periods), `step` periods apart.
    /// Returns the math channel's value for it (NaN without one).
    pub fn push(&mut self, ts: u64, step: u32, i_code: u16, v_code: u16, range: u8, dirty: bool) -> f64 {
        let idx = self.head;
        let contiguous = self.segs.back().is_some_and(|s| s.step == step && s.ts0 + (idx - s.start) * s.step as u64 == ts);
        if !contiguous {
            self.segs.push_back(Segment { start: idx, ts0: ts, step });
            if let Some(m) = &mut self.math {
                m.reset();
            }
        }
        let w = pack(i_code, v_code, range, dirty);
        self.raw[(idx % self.cap) as usize] = w;
        self.head += 1;
        let (i, v) = self.phys(w);
        let seg = *self.segs.back().unwrap();
        let mv = self.eval_math(i, v, &seg, idx);
        // the level-0 entry this sample is in: where it starts, how long it covers
        let dt = step as f64 * TS_PERIOD_S;
        if idx & ((1 << LEVEL_SHIFTS[0]) - 1) == 0 {
            self.l0 = (ts as f64 * TS_PERIOD_S, 0.0);
        }
        self.l0.1 += dt;
        self.summarize(idx, i, v, mv);
        // drop segments that ended before the oldest sample
        let oldest = self.oldest();
        while self.segs.len() > 1 && self.segs[1].start <= oldest {
            self.segs.pop_front();
        }
        mv
    }

    /// Call after a batch of pushes.
    pub fn touch(&mut self) {
        self.version += 1;
    }

    // ---- time <-> index ---------------------------------------------------

    /// (start index, end index exclusive) of segment k, clipped to the buffer.
    fn seg_bounds(&self, k: usize) -> (u64, u64) {
        let s = self.segs[k];
        let end = if k + 1 < self.segs.len() { self.segs[k + 1].start } else { self.head };
        (s.start.max(self.oldest()), end)
    }

    fn time_of(s: &Segment, idx: u64) -> f64 {
        (s.ts0 + (idx - s.start) * s.step as u64) as f64 * TS_PERIOD_S
    }

    /// First index in segment `s` at or after time t. (A sample exactly at
    /// t counts as at it despite float error in t.)
    fn index_at(s: &Segment, t: f64) -> f64 {
        let p = t / TS_PERIOD_S - s.ts0 as f64;
        s.start as f64 + (p / s.step as f64 - 1e-6).ceil()
    }

    /// Time of the oldest and newest sample, s.
    pub fn time_range(&self) -> Option<(f64, f64)> {
        if self.head == 0 || self.segs.is_empty() {
            return None;
        }
        let first = self.segs.front()?;
        let last = self.segs.back()?;
        Some((Self::time_of(first, self.oldest().max(first.start)), Self::time_of(last, self.head - 1)))
    }

    /// Index range [a, b) of segment k inside [t0, t1).
    fn span_in(&self, k: usize, t0: f64, t1: f64) -> Option<(u64, u64)> {
        let (lo, hi) = self.seg_bounds(k);
        if lo >= hi {
            return None;
        }
        let s = &self.segs[k];
        let a = Self::index_at(s, t0).max(lo as f64).min(hi as f64) as u64;
        let b = Self::index_at(s, t1).max(lo as f64).min(hi as f64) as u64;
        (a < b).then_some((a, b))
    }

    // ---- reduction ----------------------------------------------------------

    fn agg(&self, level: isize, a: u64, b: u64, acc: &mut Moments3) {
        if a >= b {
            return;
        }
        if level < 0 {
            for idx in a..b {
                let (i, v) = self.phys(self.raw[(idx % self.cap) as usize]);
                acc.add(i, v, self.mval(idx));
            }
            return;
        }
        let l = &self.levels[level as usize];
        let f = 1u64 << l.shift;
        let a2 = a.div_ceil(f) * f;
        let b2 = b / f * f;
        if a2 >= b2 {
            return self.agg(level - 1, a, b, acc);
        }
        self.agg(level - 1, a, a2, acc);
        let len = l.ring.len() as u64;
        for e in a2 / f..b2 / f {
            acc.merge(&l.ring[(e % len) as usize]);
        }
        self.agg(level - 1, b2, b, acc);
    }

    fn reduce(&self, a: u64, b: u64) -> Moments3 {
        let mut m = Moments3::default();
        self.agg(self.levels.len() as isize - 1, a, b, &mut m);
        m
    }

    /// Statistics and integrals over [t0, t1).
    pub fn stats(&self, t0: f64, t1: f64) -> SpanStats {
        let mut out = SpanStats { t0: f64::NAN, t1: f64::NAN, ..Default::default() };
        for k in 0..self.segs.len() {
            let Some((a, b)) = self.span_in(k, t0, t1) else { continue };
            let m = self.reduce(a, b);
            let s = &self.segs[k];
            let dt = s.step as f64 * TS_PERIOD_S;
            out.charge += m.i.mean * m.n() as f64 * dt;
            out.energy += m.p.mean * m.n() as f64 * dt;
            out.m.merge(&m);
            let (ta, tb) = (Self::time_of(s, a), Self::time_of(s, b - 1));
            if !(ta >= out.t0) {
                out.t0 = ta;
            }
            if !(tb <= out.t1) {
                out.t1 = tb;
            }
        }
        out
    }

    /// The sample at or just before time t: (current A, voltage V, range).
    pub fn value_at(&self, t: f64) -> Option<(f64, f64, u8, f64)> {
        for k in (0..self.segs.len()).rev() {
            let (lo, hi) = self.seg_bounds(k);
            if lo >= hi {
                continue;
            }
            let s = &self.segs[k];
            if Self::time_of(s, lo) > t {
                continue;
            }
            let idx = ((Self::index_at(s, t) as u64).saturating_sub(1)).max(lo).min(hi - 1);
            let idx = if Self::time_of(s, idx + 1) <= t && idx + 1 < hi { idx + 1 } else { idx };
            // past the end of this segment: in the gap after it
            if idx == hi - 1 && t > Self::time_of(s, hi - 1) + s.step as f64 * TS_PERIOD_S && k + 1 < self.segs.len() {
                return None;
            }
            let w = self.raw[(idx % self.cap) as usize];
            let (i, v) = self.phys(w);
            return Some((i, v, unpack(w).2, self.mval(idx)));
        }
        None
    }

    /// Every sample in [t0, t1): f(time s, current A, voltage V, range).
    pub fn for_each(&self, t0: f64, t1: f64, mut f: impl FnMut(f64, f64, f64, u8, f64)) {
        for k in 0..self.segs.len() {
            let Some((a, b)) = self.span_in(k, t0, t1) else { continue };
            let s = self.segs[k];
            for idx in a..b {
                let w = self.raw[(idx % self.cap) as usize];
                let (i, v) = self.phys(w);
                f(Self::time_of(&s, idx), i, v, unpack(w).2, self.mval(idx));
            }
        }
    }

    /// The raw samples in [t0, t1), copied out (for analysis away from the buffer's lock).
    pub fn extract(&self, t0: f64, t1: f64) -> crate::analysis::Extract {
        let mut runs = Vec::new();
        for k in 0..self.segs.len() {
            let Some((a, b)) = self.span_in(k, t0, t1) else { continue };
            let s = self.segs[k];
            let v: Vec<u32> = (a..b).map(|idx| self.raw[(idx % self.cap) as usize]).collect();
            let m: Option<Vec<f32>> = (!self.mraw.is_empty()).then(|| (a..b).map(|idx| self.mraw[(idx % self.cap) as usize]).collect());
            runs.push((Self::time_of(&s, a), s.step as f64 * TS_PERIOD_S, v, m));
        }
        crate::analysis::Extract { runs, cal: self.cal.clone() }
    }

    /// Samples in [t0, t1).
    pub fn count(&self, t0: f64, t1: f64) -> u64 {
        (0..self.segs.len()).filter_map(|k| self.span_in(k, t0, t1)).map(|(a, b)| b - a).sum()
    }

    /// [t0, t1] in `bins` columns, flat: `[layout, rows, cols, raw, then each
    /// column]`; columns t, n, then i, v, p, m (math) each min, max, mean, σ. A row with
    /// n = 0 is a gap (values NaN). raw = 1: every row is one sample (the
    /// span holds fewer samples than about two per column).
    pub fn lod(&self, t0: f64, t1: f64, bins: usize) -> Vec<f64> {
        let bins = bins.max(1);
        let w = (t1 - t0) / bins as f64;
        let mut rows: Vec<[f64; LOD_COLS]> = Vec::new();
        let gap = |t: f64| {
            let mut r = [f64::NAN; LOD_COLS];
            r[0] = t;
            r[1] = 0.0;
            r
        };
        // fewer samples than ~2 per column: send them one by one
        let mut total = 0u64;
        for k in 0..self.segs.len() {
            if let Some((a, b)) = self.span_in(k, t0, t1) {
                total += b - a;
            }
        }
        let raw = total as usize <= bins * 2;
        let mut prev_end: Option<f64> = None;
        for k in 0..self.segs.len() {
            let s = self.segs[k];
            let Some((a, b)) = self.span_in(k, t0, t1) else { continue };
            if let Some(pe) = prev_end {
                // a gap since the last segment shown
                if Self::time_of(&s, a) > pe {
                    rows.push(gap(pe));
                }
            }
            if raw {
                for idx in a..b {
                    let w = self.raw[(idx % self.cap) as usize];
                    let (i, v) = self.phys(w);
                    let p = i * v;
                    let m = self.mval(idx);
                    rows.push([Self::time_of(&s, idx), 1.0, i, i, i, 0.0, v, v, v, 0.0, p, p, p, 0.0, m, m, m, 0.0]);
                }
            } else {
                let ta = Self::time_of(&s, a);
                let tb = Self::time_of(&s, b - 1);
                let first = (((ta - t0) / w).floor().max(0.0)) as usize;
                let last = (((tb - t0) / w).floor() as usize).min(bins - 1);
                for bin in first..=last {
                    let bt0 = t0 + bin as f64 * w;
                    let Some((x, y)) = self.span_in(k, bt0, bt0 + w) else { continue };
                    let m = self.reduce(x, y);
                    if m.n() == 0 {
                        continue;
                    }
                    let c = |q: &Moments| [q.min, q.max, q.mean, q.std()];
                    let (ci, cv, cp) = (c(&m.i), c(&m.v), c(&m.p));
                    let cm = if m.m.n > 0 { c(&m.m) } else { [f64::NAN; 4] };
                    rows.push([
                        Self::time_of(&s, x), m.n() as f64,
                        ci[0], ci[1], ci[2], ci[3], cv[0], cv[1], cv[2], cv[3], cp[0], cp[1], cp[2], cp[3],
                        cm[0], cm[1], cm[2], cm[3],
                    ]);
                }
            }
            prev_end = Some(Self::time_of(&s, b - 1) + s.step as f64 * TS_PERIOD_S);
        }
        let n = rows.len();
        let mut out = Vec::with_capacity(4 + n * LOD_COLS);
        out.extend_from_slice(&[LOD_LAYOUT, n as f64, LOD_COLS as f64, raw as u8 as f64]);
        for col in 0..LOD_COLS {
            out.extend(rows.iter().map(|r| r[col]));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> Calibration {
        Calibration::default()
    }

    #[test]
    fn stats_match_direct_sums() {
        let mut b = SampleBuffer::new(1 << 20, cal());
        let mut direct = Moments3::default();
        let c = cal();
        for k in 0..300_000u64 {
            let i = ((k * 7919) % 16384) as u16;
            let v = 3300 + (k % 17) as u16;
            b.push(1000 + k, 1, i, v, 2, false);
            if k >= 1234 && k < 250_000 {
                direct.add(c.current(i, 2), c.voltage(v), f64::NAN);
            }
        }
        let t = |k: u64| (1000 + k) as f64 * TS_PERIOD_S;
        let s = b.stats(t(1234), t(250_000));
        assert_eq!(s.m.n(), direct.n());
        assert!((s.m.i.mean - direct.i.mean).abs() < 1e-12);
        assert!((s.m.i.m2 - direct.i.m2).abs() / direct.i.m2 < 1e-9);
        assert_eq!(s.m.i.max, direct.i.max);
        assert!((s.m.p.mean - direct.p.mean).abs() < 1e-12);
        // integral = mean × n × 500 ns
        assert!((s.charge - direct.i.mean * direct.n() as f64 * 500e-9).abs() < 1e-15);
    }

    #[test]
    fn segments_gaps_lod_and_ring() {
        let mut b = SampleBuffer::new(1 << 12, cal()); // 4096 samples
        for k in 0..3000u64 {
            b.push(k, 1, 100, 1000, 0, false);
        }
        for k in 0..3000u64 {
            b.push(10_000 + k, 1, 200, 1000, 0, false); // after a gap
        }
        // the ring holds the last 4096: 1096 of the first segment, all of the second
        assert_eq!(b.len(), 4096);
        let (ta, tb) = b.time_range().unwrap();
        assert!((ta - 1904.0 * TS_PERIOD_S).abs() < 1e-12);
        assert!((tb - 12_999.0 * TS_PERIOD_S).abs() < 1e-12);
        let l = b.lod(0.0, 13_000.0 * TS_PERIOD_S, 100);
        assert_eq!(l[0], LOD_LAYOUT);
        assert_eq!(l[3], 0.0); // not raw
        let rows = l[1] as usize;
        let n_col = &l[4 + rows..4 + 2 * rows];
        assert!(n_col.contains(&0.0)); // the gap row
        assert_eq!(n_col.iter().sum::<f64>(), 4096.0);
        // zoomed in: individual samples
        let l = b.lod(10_000.0 * TS_PERIOD_S, 10_010.0 * TS_PERIOD_S, 100);
        assert_eq!(l[3], 1.0);
        assert_eq!(l[1], 10.0);
        // value in the gap: none; inside: the sample
        assert!(b.value_at(5000.0 * TS_PERIOD_S).is_none());
        let (i, _, _, _) = b.value_at(10_005.0 * TS_PERIOD_S).unwrap();
        assert!((i - 200.0 * 2.0 / 16384.0).abs() < 1e-12);
    }

    #[test]
    fn math_channel_any_time() {
        let mut b = SampleBuffer::new(1 << 16, cal());
        for k in 0..10_000u64 {
            b.push(k, 1, 8192, 3300, 0, false); // 1 A, 3.3 V
        }
        let t1 = 10_000.0 * TS_PERIOD_S;
        assert_eq!(b.stats(0.0, t1).m.m.n, 0); // none yet
        // set afterwards: computed over what's held
        b.set_math(Some("P*1000")).unwrap();
        let s = b.stats(0.0, t1);
        assert_eq!(s.m.m.n, 10_000);
        assert!((s.m.m.mean - 3300.0).abs() < 1e-9);
        // and for new samples, with state: integ(I) grows by 1 A × 500 ns per sample
        b.set_math(Some("integ(I)")).unwrap();
        let m = b.push(10_000, 1, 8192, 3300, 0, false);
        assert!((m - 10_001.0 * 500e-9).abs() < 1e-12);
        let (_, _, _, mv) = b.value_at(5000.0 * TS_PERIOD_S).unwrap();
        assert!((mv - 5001.0 * 500e-9).abs() < 1e-9);
        // lod carries it
        let l = b.lod(0.0, t1, 10);
        let rows = l[1] as usize;
        let m_mean = &l[4 + 16 * rows..4 + 17 * rows];
        assert!(m_mean.iter().all(|x| x.is_finite()));
        assert!(b.set_math(Some("I +")).is_err());
    }

    #[test]
    fn recalibration_rebuilds() {
        let mut b = SampleBuffer::new(1 << 16, cal());
        for k in 0..10_000u64 {
            b.push(k, 1, 8192, 1000, 0, false);
        }
        let t1 = 10_000.0 * TS_PERIOD_S;
        assert!((b.stats(0.0, t1).m.i.mean - 1.0).abs() < 1e-12);
        let mut c = cal();
        c.i_fs[0] = 4.0;
        b.set_calibration(c);
        assert!((b.stats(0.0, t1).m.i.mean - 2.0).abs() < 1e-12);
    }
}

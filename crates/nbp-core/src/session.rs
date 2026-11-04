//! The session log: interval summaries of the whole session, in bounded
//! memory, for logging over days.
//!
//! Entries (one per `interval`, default 100 ms) carry, for I, V, P and the
//! math channel, count / mean / M2 / min / max, plus the time their samples
//! cover, so statistics and integrals over any span stay exact. They are
//! built from the sample buffer's 64-sample summaries, so the log costs next
//! to nothing per sample.
//!
//! Memory is bounded in three tiers: the newest `FINE_CAP` entries at the
//! interval; older ones merged ten at a time (`MID_CAP` of those); older
//! still merged ten more (unbounded, but ~1 MB a day). Days of logging
//! fit in a few hundred MB, never more.
//!
//! The .nblog file holds every fine entry (see [`LogWriter`]); a file cut
//! short reads to its last whole batch.

use crate::buffer::{Moments, Moments3};
use serde::Serialize;
use std::collections::VecDeque;

pub const FINE_CAP: usize = 1_000_000;
pub const MID_CAP: usize = 1_000_000;
const MERGE: usize = 10;

/// One channel of an entry, compact.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ch {
    pub n: u32,
    pub mean: f64,
    pub m2: f32,
    pub min: f32,
    pub max: f32,
}

impl Ch {
    fn from(m: &Moments) -> Ch {
        if m.n == 0 {
            return Ch::default();
        }
        Ch { n: m.n as u32, mean: m.mean, m2: m.m2 as f32, min: m.min as f32, max: m.max as f32 }
    }

    pub fn moments(&self) -> Moments {
        if self.n == 0 {
            return Moments::default();
        }
        Moments { n: self.n as u64, mean: self.mean, m2: self.m2 as f64, min: self.min as f64, max: self.max as f64 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LogEntry {
    /// time of the first sample, s (FPGA time, as the sample buffer)
    pub t0: f64,
    /// time the samples cover (n × their spacing), s
    pub dur: f64,
    /// I, V, P, math
    pub ch: [Ch; 4],
}

impl LogEntry {
    fn from_m(t0: f64, dur: f64, m: &Moments3) -> LogEntry {
        LogEntry { t0, dur, ch: [Ch::from(&m.i), Ch::from(&m.v), Ch::from(&m.p), Ch::from(&m.m)] }
    }

    /// An entry from moments (tests, exports).
    pub fn from_m_pub(t0: f64, dur: f64, m: &Moments3) -> LogEntry {
        Self::from_m(t0, dur, m)
    }

    pub fn moments3(&self) -> Moments3 {
        Moments3 { i: self.ch[0].moments(), v: self.ch[1].moments(), p: self.ch[2].moments(), m: self.ch[3].moments() }
    }

    pub fn n(&self) -> u64 {
        self.ch[0].n as u64
    }

    fn merge(&mut self, o: &LogEntry) {
        if self.n() == 0 {
            *self = *o;
            return;
        }
        let mut m = self.moments3();
        let om = o.moments3();
        merge3(&mut m, &om);
        *self = LogEntry::from_m(self.t0, self.dur + o.dur, &m);
    }

    // file layout: t0 f64, dur f64, 4 × (n u32, mean f64, m2 f32, min f32, max f32) = 112 bytes
    pub const BYTES: usize = 16 + 4 * 24;

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.t0.to_le_bytes());
        out.extend_from_slice(&self.dur.to_le_bytes());
        for c in &self.ch {
            out.extend_from_slice(&c.n.to_le_bytes());
            out.extend_from_slice(&c.mean.to_le_bytes());
            out.extend_from_slice(&c.m2.to_le_bytes());
            out.extend_from_slice(&c.min.to_le_bytes());
            out.extend_from_slice(&c.max.to_le_bytes());
        }
    }

    fn read(b: &[u8]) -> LogEntry {
        let f64_at = |o: usize| f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        let f32_at = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let mut e = LogEntry { t0: f64_at(0), dur: f64_at(8), ch: [Ch::default(); 4] };
        for (k, c) in e.ch.iter_mut().enumerate() {
            let o = 16 + k * 24;
            *c = Ch { n: u32_at(o), mean: f64_at(o + 4), m2: f32_at(o + 12), min: f32_at(o + 16), max: f32_at(o + 20) };
        }
        e
    }
}

fn merge3(a: &mut Moments3, b: &Moments3) {
    // Moments3::merge is private to the buffer; the same per channel
    for (x, y) in [(&mut a.i, &b.i), (&mut a.v, &b.v), (&mut a.p, &b.p), (&mut a.m, &b.m)] {
        x.merge(y);
    }
}

/// A tier: entries, plus the one being merged from the tier below, and
/// summaries of runs of its entries so a query over a long span doesn't
/// read every entry.
#[derive(Default)]
struct Tier {
    entries: VecDeque<LogEntry>,
    building: Option<(LogEntry, usize)>,
    /// index (since the tier began) of `entries[0]`
    base: u64,
    /// `BLOCK_SIZES[k]` entries per block of `levels[k]`
    levels: [Level; BLOCK_LEVELS],
    /// each entry starts after the one before: ranges can be searched
    /// (false after the clock went back: queries read every entry)
    sorted: bool,
    /// time of the newest entry pushed (for `sorted`)
    last_t0: f64,
}

/// Summaries of runs of a tier's entries: 16, 256, 4096, 65 536 of them.
const BLOCK_LEVELS: usize = 4;
const BLOCK_SIZES: [u64; BLOCK_LEVELS] = [16, 256, 4096, 65_536];

#[derive(Default)]
struct Level {
    /// complete blocks, oldest first; a block starts at a multiple of its size
    blocks: VecDeque<Block>,
    /// the block being filled
    cur: Option<Block>,
}

/// Several consecutive entries merged: what a query needs of them.
#[derive(Debug, Clone, Copy)]
struct Block {
    /// index of its first entry in the tier
    start: u64,
    /// entries in it
    count: u64,
    m: Moments3,
    /// Σ mean(I)·dur, Σ mean(P)·dur
    charge: f64,
    energy: f64,
    /// the first entry's time and duration, the last's
    t0: f64,
    first_dur: f64,
    t_last: f64,
    last_dur: f64,
    /// a gap between two of its entries (lod draws gaps: it can't merge this block)
    gap: bool,
}

/// A gap in the log: more than 1.5 entries' time with nothing (as the lod draws it).
fn is_gap(prev_end: f64, prev_dur: f64, t0: f64, dur: f64, interval: f64) -> bool {
    t0 - prev_end > 1.5 * prev_dur.max(dur) && t0 - prev_end > 1.5 * interval
}

impl Block {
    fn of(e: &LogEntry, start: u64) -> Block {
        let m = e.moments3();
        Block {
            start,
            count: 1,
            charge: m.i.mean * e.dur,
            energy: m.p.mean * e.dur,
            m,
            t0: e.t0,
            first_dur: e.dur,
            t_last: e.t0,
            last_dur: e.dur,
            gap: false,
        }
    }

    fn end(&self) -> f64 {
        self.t_last + self.last_dur
    }

    /// `o` follows this block
    fn append(&mut self, o: &Block, interval: f64) {
        self.gap |= o.gap || is_gap(self.end(), self.last_dur, o.t0, o.first_dur, interval);
        merge3(&mut self.m, &o.m);
        self.count += o.count;
        self.charge += o.charge;
        self.energy += o.energy;
        self.t_last = o.t_last;
        self.last_dur = o.last_dur;
    }
}

/// What a range walk hands out: an entry, or a block of them.
enum Item<'a> {
    Entry(&'a LogEntry),
    Block(&'a Block),
}

impl Item<'_> {
    fn t0(&self) -> f64 {
        match self {
            Item::Entry(e) => e.t0,
            Item::Block(b) => b.t0,
        }
    }
    fn first_dur(&self) -> f64 {
        match self {
            Item::Entry(e) => e.dur,
            Item::Block(b) => b.first_dur,
        }
    }
    /// the last entry's time and duration
    fn last(&self) -> (f64, f64) {
        match self {
            Item::Entry(e) => (e.t0, e.dur),
            Item::Block(b) => (b.t_last, b.last_dur),
        }
    }
    fn count(&self) -> u64 {
        match self {
            Item::Entry(_) => 1,
            Item::Block(b) => b.count,
        }
    }
}

impl Tier {
    fn new() -> Tier {
        Tier { sorted: true, last_t0: f64::NEG_INFINITY, ..Default::default() }
    }

    fn push(&mut self, e: LogEntry, interval: f64) {
        if e.t0 < self.last_t0 {
            self.sorted = false;
        }
        self.last_t0 = e.t0;
        let idx = self.base + self.entries.len() as u64;
        self.entries.push_back(e);
        // into the block being filled at each level; a full one goes up a level
        let mut done = Some(Block::of(&e, idx));
        for (k, l) in self.levels.iter_mut().enumerate() {
            let Some(b) = done.take() else { break };
            match &mut l.cur {
                Some(c) => c.append(&b, interval),
                None => l.cur = Some(b),
            }
            if l.cur.as_ref().is_some_and(|c| c.count == BLOCK_SIZES[k]) {
                let c = l.cur.take().unwrap();
                l.blocks.push_back(c);
                done = Some(c);
            }
        }
    }

    fn pop_front(&mut self) -> Option<LogEntry> {
        let e = self.entries.pop_front()?;
        self.base += 1;
        // a block missing its first entries can't stand for them any more
        for l in &mut self.levels {
            while l.blocks.front().is_some_and(|b| b.start < self.base) {
                l.blocks.pop_front();
            }
        }
        Some(e)
    }

    /// the complete block of `level` starting at entry `i`, if there is one
    fn block_at(&self, level: usize, i: u64) -> Option<&Block> {
        let bl = &self.levels[level].blocks;
        let first = bl.front()?.start;
        if i < first {
            return None;
        }
        let b = bl.get(((i - first) / BLOCK_SIZES[level]) as usize)?;
        (b.start == i).then_some(b)
    }

    /// The entries overlapping [t0, t1], oldest first, as the largest blocks
    /// that `usable` accepts and entries where none is.
    fn walk<'a>(&'a self, t0: f64, t1: f64, usable: &impl Fn(&Block) -> bool, f: &mut impl FnMut(Item<'a>)) {
        if !self.sorted {
            for e in self.entries.iter().filter(|e| !(e.t0 + e.dur < t0 || e.t0 > t1)) {
                f(Item::Entry(e));
            }
            return;
        }
        let lo = self.entries.partition_point(|e| e.t0 + e.dur < t0);
        let hi = self.entries.partition_point(|e| e.t0 <= t1);
        let (mut i, end) = (self.base + lo as u64, self.base + hi.max(lo) as u64);
        'next: while i < end {
            for k in (0..BLOCK_LEVELS).rev() {
                let n = BLOCK_SIZES[k];
                if i % n == 0 && i + n <= end {
                    if let Some(b) = self.block_at(k, i).filter(|b| usable(b)) {
                        f(Item::Block(b));
                        i += n;
                        continue 'next;
                    }
                }
            }
            f(Item::Entry(&self.entries[(i - self.base) as usize]));
            i += 1;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct SessionInfo {
    pub has_data: bool,
    pub t_start: f64,
    pub t_end: f64,
    /// s
    pub interval: f64,
    pub entries: u64,
    pub version: u64,
    /// bytes of memory the log uses, about
    pub bytes: u64,
}

pub struct SessionLog {
    interval: f64,
    /// the interval being built: its index (t0 / interval), first sample's time, moments, time covered
    cur: Option<(i64, f64, Moments3, f64)>,
    fine: Tier,
    mid: Tier,
    coarse: Tier,
    fine_cap: usize,
    /// mid entries before merging into coarse
    mid_cap: usize,
    /// fine entries not yet taken for the log file
    pending: Vec<LogEntry>,
    version: u64,
    total: u64,
}

impl SessionLog {
    pub fn new(interval: f64) -> Self {
        Self::with_cap(interval, FINE_CAP)
    }

    /// `fine_cap` fine entries before merging (a whole file opened: all of them)
    pub fn with_cap(interval: f64, fine_cap: usize) -> Self {
        Self {
            interval: interval.max(1e-4),
            cur: None,
            fine: Tier::new(),
            mid: Tier::new(),
            coarse: Tier::new(),
            fine_cap,
            mid_cap: MID_CAP,
            pending: Vec::new(),
            version: 0,
            total: 0,
        }
    }

    pub fn interval(&self) -> f64 {
        self.interval
    }

    /// A 64-sample summary from the buffer: its first sample's time, the
    /// time it covers, its moments.
    pub fn add(&mut self, t0: f64, dur: f64, m: &Moments3) {
        let k = (t0 / self.interval).floor() as i64;
        match &mut self.cur {
            Some((ck, _, acc, d)) if *ck == k => {
                merge3(acc, m);
                *d += dur;
            }
            _ => {
                self.close();
                self.cur = Some((k, t0, *m, dur));
            }
        }
    }

    /// The interval being built ends (the next began, or the stream stopped).
    pub fn close(&mut self) {
        if let Some((_, t0, m, d)) = self.cur.take() {
            if m.n() > 0 {
                let e = LogEntry::from_m(t0, d, &m);
                self.pending.push(e);
                self.push_fine(e);
            }
        }
    }

    fn push_fine(&mut self, e: LogEntry) {
        let iv = self.interval;
        self.fine.push(e, iv);
        self.total += 1;
        self.version += 1;
        while self.fine.entries.len() > self.fine_cap {
            let old = self.fine.pop_front().unwrap();
            Self::fold(&mut self.mid, old, iv);
        }
        while self.mid.entries.len() > self.mid_cap {
            let old = self.mid.pop_front().unwrap();
            Self::fold(&mut self.coarse, old, iv);
        }
    }

    /// An entry leaving the tier below, merged MERGE at a time into `t`
    /// (`interval`: the log's, which gaps are measured against).
    fn fold(t: &mut Tier, e: LogEntry, interval: f64) {
        match &mut t.building {
            Some((b, k)) => {
                b.merge(&e);
                *k += 1;
                if *k == MERGE {
                    let done = t.building.take().unwrap().0;
                    t.push(done, interval);
                }
            }
            None => t.building = Some((e, 1)),
        }
    }

    /// Everything overlapping [t0, t1], oldest first: each tier's entries (as
    /// blocks where `usable` takes them) and the entries being merged.
    fn walk<'a>(&'a self, t0: f64, t1: f64, usable: impl Fn(&Block) -> bool, mut f: impl FnMut(Item<'a>)) {
        let overlaps = |e: &LogEntry| !(e.t0 + e.dur < t0 || e.t0 > t1);
        for t in [&self.coarse, &self.mid] {
            t.walk(t0, t1, &usable, &mut f);
            if let Some((e, _)) = t.building.as_ref().filter(|(e, _)| overlaps(e)) {
                f(Item::Entry(e));
            }
        }
        self.fine.walk(t0, t1, &usable, &mut f);
    }

    /// The newest `n` fine entries (oldest first).
    pub fn last(&self, n: usize) -> Vec<LogEntry> {
        let k = self.fine.entries.len().saturating_sub(n);
        self.fine.entries.iter().skip(k).copied().collect()
    }

    /// Fine entries finished since the last call (for the log file).
    pub fn take_pending(&mut self) -> Vec<LogEntry> {
        std::mem::take(&mut self.pending)
    }

    pub fn clear(&mut self) {
        let (iv, cap) = (self.interval, self.fine_cap);
        *self = Self::with_cap(iv, cap);
    }

    /// Every entry held, oldest first (coarse, mid, fine; the ones being merged in their place).
    fn all(&self) -> impl Iterator<Item = &LogEntry> {
        let c = self.coarse.entries.iter().chain(self.coarse.building.iter().map(|b| &b.0));
        let m = self.mid.entries.iter().chain(self.mid.building.iter().map(|b| &b.0));
        c.chain(m).chain(self.fine.entries.iter())
    }

    pub fn info(&self) -> SessionInfo {
        let first = self.all().next();
        let last = self.fine.entries.back();
        let n = self.coarse.entries.len() + self.mid.entries.len() + self.fine.entries.len();
        SessionInfo {
            has_data: first.is_some(),
            t_start: first.map_or(0.0, |e| e.t0),
            t_end: last.map_or(0.0, |e| e.t0 + e.dur),
            interval: self.interval,
            entries: self.total,
            version: self.version,
            bytes: (n * std::mem::size_of::<LogEntry>()) as u64,
        }
    }

    /// Statistics and integrals over the entries overlapping [t0, t1] (whole entries).
    pub fn stats(&self, t0: f64, t1: f64) -> crate::buffer::SpanStats {
        let mut out = crate::buffer::SpanStats { t0: f64::NAN, t1: f64::NAN, ..Default::default() };
        self.walk(t0, t1, |_| true, |it| {
            let (start, end) = match it {
                Item::Entry(e) => {
                    if e.n() == 0 {
                        return;
                    }
                    let m = e.moments3();
                    out.charge += m.i.mean * e.dur;
                    out.energy += m.p.mean * e.dur;
                    merge3(&mut out.m, &m);
                    (e.t0, e.t0 + e.dur)
                }
                Item::Block(b) => {
                    out.charge += b.charge;
                    out.energy += b.energy;
                    merge3(&mut out.m, &b.m);
                    (b.t0, b.end())
                }
            };
            if !(start >= out.t0) {
                out.t0 = start;
            }
            if !(end <= out.t1) {
                out.t1 = end;
            }
        });
        out
    }

    /// [t0, t1] in `bins` columns, in the sample buffer's flat lod layout
    /// (rows: t, n, then i, v, p, m each min, max, mean, σ; n = 0 a gap).
    pub fn lod(&self, t0: f64, t1: f64, bins: usize) -> Vec<f64> {
        use crate::buffer::{LOD_COLS, LOD_LAYOUT};
        let bins = bins.max(1);
        let w = (t1 - t0) / bins as f64;
        let mut rows: Vec<[f64; LOD_COLS]> = Vec::new();
        let mut cur: Option<(usize, f64, Moments3)> = None;
        let mut prev_end: Option<(f64, f64)> = None; // end, entry length
        let emit = |rows: &mut Vec<[f64; LOD_COLS]>, t: f64, m: &Moments3| {
            let c = |q: &Moments| if q.n > 0 { [q.min, q.max, q.mean, q.std()] } else { [f64::NAN; 4] };
            let (a, b, p, mm) = (c(&m.i), c(&m.v), c(&m.p), c(&m.m));
            rows.push([
                t, m.n() as f64, a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3], p[0], p[1], p[2], p[3], mm[0], mm[1], mm[2], mm[3],
            ]);
        };
        let mut shown = 0usize;
        let bin_of = |t: f64| ((t - t0) / w).floor().max(0.0) as usize;
        // a block stands for its entries when they all fall in one column with no gap
        let usable = |b: &Block| !b.gap && bin_of(b.t0) == bin_of(b.t_last);
        self.walk(t0, t1, usable, |it| {
            if let Item::Entry(e) = it {
                if e.n() == 0 {
                    return;
                }
            }
            shown += it.count() as usize;
            if let Some((end, len)) = prev_end {
                if is_gap(end, len, it.t0(), it.first_dur(), self.interval) {
                    if let Some((_, t, m)) = cur.take() {
                        emit(&mut rows, t, &m);
                    }
                    let mut g = [f64::NAN; LOD_COLS];
                    g[0] = end;
                    g[1] = 0.0;
                    rows.push(g);
                }
            }
            let (tl, dl) = it.last();
            prev_end = Some((tl + dl, dl));
            let bin = bin_of(it.t0());
            let m = match &it {
                Item::Entry(e) => e.moments3(),
                Item::Block(b) => b.m,
            };
            match &mut cur {
                Some((b, _, acc)) if *b == bin => merge3(acc, &m),
                _ => {
                    if let Some((_, t, m0)) = cur.take() {
                        emit(&mut rows, t, &m0);
                    }
                    cur = Some((bin, it.t0(), m));
                }
            }
        });
        if let Some((_, t, m)) = cur.take() {
            emit(&mut rows, t, &m);
        }
        let n = rows.len();
        let mut out = Vec::with_capacity(4 + n * LOD_COLS);
        // "raw": fewer entries than columns, each its own row
        out.extend_from_slice(&[LOD_LAYOUT, n as f64, LOD_COLS as f64, (shown <= bins) as u8 as f64]);
        for col in 0..LOD_COLS {
            out.extend(rows.iter().map(|r| r[col]));
        }
        out
    }

    /// The full scan `lod` replaced: the tests' reference.
    /// (rows: t, n, then i, v, p, m each min, max, mean, σ; n = 0 a gap).
    #[cfg(test)]
    fn lod_scan(&self, t0: f64, t1: f64, bins: usize) -> Vec<f64> {
        use crate::buffer::{LOD_COLS, LOD_LAYOUT};
        let bins = bins.max(1);
        let w = (t1 - t0) / bins as f64;
        let mut rows: Vec<[f64; LOD_COLS]> = Vec::new();
        let mut cur: Option<(usize, f64, Moments3)> = None;
        let mut prev_end: Option<(f64, f64)> = None; // end, entry length
        let emit = |rows: &mut Vec<[f64; LOD_COLS]>, t: f64, m: &Moments3| {
            let c = |q: &Moments| if q.n > 0 { [q.min, q.max, q.mean, q.std()] } else { [f64::NAN; 4] };
            let (a, b, p, mm) = (c(&m.i), c(&m.v), c(&m.p), c(&m.m));
            rows.push([
                t, m.n() as f64, a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3], p[0], p[1], p[2], p[3], mm[0], mm[1], mm[2], mm[3],
            ]);
        };
        let mut shown = 0usize;
        for e in self.all() {
            if e.t0 + e.dur < t0 || e.t0 > t1 || e.n() == 0 {
                continue;
            }
            shown += 1;
            // a gap: more than 1.5 entries' time with nothing
            if let Some((end, len)) = prev_end {
                if e.t0 - end > 1.5 * len.max(e.dur) && e.t0 - end > 1.5 * self.interval {
                    if let Some((_, t, m)) = cur.take() {
                        emit(&mut rows, t, &m);
                    }
                    let mut g = [f64::NAN; LOD_COLS];
                    g[0] = end;
                    g[1] = 0.0;
                    rows.push(g);
                }
            }
            prev_end = Some((e.t0 + e.dur, e.dur));
            let bin = ((e.t0 - t0) / w).floor().max(0.0) as usize;
            let m = e.moments3();
            match &mut cur {
                Some((b, _, acc)) if *b == bin => merge3(acc, &m),
                _ => {
                    if let Some((_, t, m0)) = cur.take() {
                        emit(&mut rows, t, &m0);
                    }
                    cur = Some((bin, e.t0, m));
                }
            }
        }
        if let Some((_, t, m)) = cur.take() {
            emit(&mut rows, t, &m);
        }
        let n = rows.len();
        let mut out = Vec::with_capacity(4 + n * LOD_COLS);
        // "raw": fewer entries than columns, each its own row
        out.extend_from_slice(&[LOD_LAYOUT, n as f64, LOD_COLS as f64, (shown <= bins) as u8 as f64]);
        for col in 0..LOD_COLS {
            out.extend(rows.iter().map(|r| r[col]));
        }
        out
    }

    /// A log file's entries, all kept (no merging), for viewing it.
    pub fn from_entries(interval: f64, entries: Vec<LogEntry>) -> Self {
        let mut s = Self::with_cap(interval, entries.len().max(1));
        for e in entries {
            s.push_fine(e);
        }
        s
    }
}

// ---- the .nblog file ------------------------------------------------------------

pub const LOG_MAGIC: &[u8; 8] = b"NBLOG\0\x01\0";

fn record(tag: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
}

/// Encodes a log file: magic, 'H' header JSON, then 'L' batches of entries,
/// then 'E' at the end. Bytes only; the app writes them.
pub struct LogWriter {
    out: Vec<u8>,
    pub entries: u64,
    pub bytes: u64,
}

impl LogWriter {
    pub fn new(header_json: &str) -> Self {
        let mut out = LOG_MAGIC.to_vec();
        record(b'H', header_json.as_bytes(), &mut out);
        Self { out, entries: 0, bytes: 0 }
    }

    pub fn add(&mut self, es: &[LogEntry]) {
        if es.is_empty() {
            return;
        }
        let mut p = Vec::with_capacity(es.len() * LogEntry::BYTES);
        for e in es {
            e.write(&mut p);
        }
        record(b'L', &p, &mut self.out);
        self.entries += es.len() as u64;
    }

    pub fn take(&mut self) -> Vec<u8> {
        let b = std::mem::take(&mut self.out);
        self.bytes += b.len() as u64;
        b
    }

    pub fn finish(mut self) -> Vec<u8> {
        let end = format!("{{\"entries\":{}}}", self.entries);
        record(b'E', end.as_bytes(), &mut self.out);
        self.take()
    }
}

pub struct LogFile {
    pub header_json: String,
    pub entries: Vec<LogEntry>,
    pub truncated: bool,
}

impl LogFile {
    pub fn parse(b: &[u8]) -> Result<LogFile, String> {
        if b.len() < 8 || &b[..8] != LOG_MAGIC {
            return Err("not a Nebulosa log (.nblog)".into());
        }
        let mut f = LogFile { header_json: "{}".into(), entries: Vec::new(), truncated: true };
        let mut p = 8;
        while p + 5 <= b.len() {
            let tag = b[p];
            let len = u32::from_le_bytes([b[p + 1], b[p + 2], b[p + 3], b[p + 4]]) as usize;
            if p + 5 + len > b.len() {
                break;
            }
            let d = &b[p + 5..p + 5 + len];
            p += 5 + len;
            match tag {
                b'H' => f.header_json = String::from_utf8_lossy(d).into_owned(),
                b'L' => f.entries.extend(d.chunks_exact(LogEntry::BYTES).map(LogEntry::read)),
                b'E' => f.truncated = false,
                _ => {}
            }
        }
        Ok(f)
    }

    /// The interval the entries were logged at (the median spacing).
    pub fn interval(&self) -> f64 {
        let mut d: Vec<f64> = self.entries.windows(2).map(|w| w[1].t0 - w[0].t0).filter(|x| *x > 0.0).take(1000).collect();
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        d.get(d.len() / 2).copied().unwrap_or(0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m3(x: f64, n: u64) -> Moments3 {
        let mut m = Moments3::default();
        for _ in 0..n {
            m.add(x, 1.0, f64::NAN);
        }
        m
    }

    #[test]
    fn intervals_tiers_and_exact_integrals() {
        let mut s = SessionLog::with_cap(0.1, 50);
        // 1000 s of 64-sample blocks of 1 ms each, current 2 A: 10 000 entries
        for k in 0..1_000_000u64 {
            s.add(k as f64 * 1e-3, 1e-3, &m3(2.0, 64));
        }
        s.close();
        let info = s.info();
        assert_eq!(info.entries, 10_000);
        // bounded: 50 fine, the rest merged
        assert!(s.fine.entries.len() <= 50);
        let held = s.coarse.entries.len() + s.mid.entries.len() + s.fine.entries.len();
        assert!(held < 1100, "held {held}");
        // integrals over everything exact despite the merging
        let st = s.stats(0.0, 2000.0);
        assert!((st.charge - 2000.0).abs() < 1e-6, "charge {}", st.charge);
        assert_eq!(st.m.n(), 64_000_000);
        // lod covers it, no gaps
        let l = s.lod(0.0, 1000.0, 100);
        let rows = l[1] as usize;
        assert!(l[4 + rows..4 + 2 * rows].iter().all(|&n| n > 0.0));
    }

    #[test]
    fn gaps_show() {
        let mut s = SessionLog::new(0.1);
        for k in 0..100u64 {
            s.add(k as f64 * 0.01, 0.01, &m3(1.0, 64));
        }
        for k in 0..100u64 {
            s.add(100.0 + k as f64 * 0.01, 0.01, &m3(1.0, 64)); // 99 s later
        }
        s.close();
        let l = s.lod(0.0, 101.0, 2000);
        let rows = l[1] as usize;
        assert!(l[4 + rows..4 + 2 * rows].contains(&0.0));
    }

    #[test]
    fn file_roundtrip_and_cut_short() {
        let mut s = SessionLog::new(0.1);
        let mut w = LogWriter::new("{\"x\":1}");
        let mut file = w.take();
        for k in 0..5000u64 {
            s.add(k as f64 * 1e-3, 1e-3, &m3(k as f64, 64));
            w.add(&s.take_pending());
            file.extend(w.take());
        }
        s.close();
        w.add(&s.take_pending());
        file.extend(w.take());
        file.extend(w.finish());
        let f = LogFile::parse(&file).unwrap();
        assert!(!f.truncated);
        assert_eq!(f.entries.len(), 50);
        assert!((f.interval() - 0.1).abs() < 1e-9);
        assert_eq!(f.entries[3], s.fine.entries[3]);
        let cut = LogFile::parse(&file[..file.len() - 200]).unwrap();
        assert!(cut.truncated && cut.entries.len() < 50);
    }

    /// the full scans these replaced: the reference
    fn stats_scan(s: &SessionLog, t0: f64, t1: f64) -> crate::buffer::SpanStats {
        let mut out = crate::buffer::SpanStats { t0: f64::NAN, t1: f64::NAN, ..Default::default() };
        for e in s.all() {
            if e.t0 + e.dur < t0 || e.t0 > t1 || e.n() == 0 {
                continue;
            }
            let m = e.moments3();
            out.charge += m.i.mean * e.dur;
            out.energy += m.p.mean * e.dur;
            merge3(&mut out.m, &m);
            if !(e.t0 >= out.t0) {
                out.t0 = e.t0;
            }
            if !(e.t0 + e.dur <= out.t1) {
                out.t1 = e.t0 + e.dur;
            }
        }
        out
    }

    fn close(a: f64, b: f64) -> bool {
        a == b || (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-12)
    }

    /// a log with gaps, uneven entries and all three tiers in use
    fn gappy() -> SessionLog {
        let mut s = SessionLog::with_cap(0.1, 30_000);
        s.mid_cap = 20_000;
        let mut t = 0.0;
        let mut x: u64 = 12345;
        for k in 0..400_000u64 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let r = (x >> 33) as f64 / (1u64 << 31) as f64;
            // now and then a pause in the stream
            if k % 7919 == 0 {
                t += 5.0 + 50.0 * r;
            }
            let n = 64 * (1 + (k % 3));
            let mut m = Moments3::default();
            for j in 0..4 {
                m.add(0.001 + r * 0.01 + j as f64 * 1e-4, 3.3 - r * 0.1, f64::NAN);
            }
            s.add(t, n as f64 * 5e-7, &m);
            t += 0.1;
        }
        s.close();
        assert!(s.mid.entries.len() > 0 && s.coarse.entries.len() > 0, "all tiers in use");
        s
    }

    #[test]
    fn blocks_give_the_same_answers_as_a_full_scan() {
        let s = gappy();
        let (a, b) = (s.all().next().unwrap().t0, s.fine.entries.back().unwrap().t0 + 1.0);
        for (t0, t1) in [(a, b), (a - 10.0, b + 10.0), (b - 61.0, b), (a + 1234.5, a + 9876.5), (b - 3000.0, b - 2999.0), (b + 5.0, b + 6.0)] {
            let (f, r) = (s.stats(t0, t1), stats_scan(&s, t0, t1));
            assert!(close(f.charge, r.charge) && close(f.energy, r.energy), "integrals {t0}..{t1}");
            assert!(close(f.t0, r.t0) && close(f.t1, r.t1), "span {t0}..{t1}");
            for (x, y) in [(&f.m.i, &r.m.i), (&f.m.v, &r.m.v), (&f.m.p, &r.m.p)] {
                assert_eq!(x.n, y.n);
                assert!(close(x.mean, y.mean) && close(x.min, y.min) && close(x.max, y.max), "moments {t0}..{t1}: {x:?} vs {y:?}");
                assert!((x.m2 - y.m2).abs() <= 1e-6 * y.m2.abs().max(1e-18), "m2 {t0}..{t1}");
            }
            for bins in [1, 37, 800, 5000] {
                let (l, r) = (s.lod(t0, t1, bins), s.lod_scan(t0, t1, bins));
                assert_eq!(l.len(), r.len(), "lod rows {t0}..{t1} / {bins}");
                for (k, (x, y)) in l.iter().zip(&r).enumerate() {
                    assert!(close(*x, *y) || (x - y).abs() < 1e-9, "lod {t0}..{t1} / {bins} at {k}: {x} vs {y}");
                }
            }
        }
    }

    #[test]
    fn a_clock_going_back_still_answers() {
        let mut s = SessionLog::with_cap(0.1, 1000);
        let mut m = Moments3::default();
        m.add(0.002, 3.3, f64::NAN);
        for k in 0..500 {
            s.add(k as f64 * 0.1, 0.05, &m);
        }
        for k in 0..500 {
            s.add(k as f64 * 0.1 + 0.01, 0.05, &m); // restarted at 0
        }
        s.close();
        let (f, r) = (s.stats(0.0, 100.0), stats_scan(&s, 0.0, 100.0));
        assert_eq!(f.m.i.n, r.m.i.n);
        let (l, r) = (s.lod(0.0, 100.0, 50), s.lod_scan(0.0, 100.0, 50));
        assert!(l.len() == r.len() && l.iter().zip(&r).all(|(x, y)| close(*x, *y)));
    }
}

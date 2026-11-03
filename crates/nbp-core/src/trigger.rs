//! Trigger and event detection on the full-rate samples.
//!
//! The source (I, V or P), optionally smoothed by a moving average, goes
//! through one of these:
//!
//! - **edge**: crosses `level` going up / down / either. Re-arms only after
//!   the signal has been `hysteresis` past the level on the other side.
//! - **window**: enters and / or exits the band [low, high] (leaving takes
//!   `hysteresis` beyond the band).
//! - **pulse**: a pulse above (polarity high) or below (low) `level`, whose
//!   width is greater than / less than / within [min, max] / any. Fires when
//!   the pulse ends; the event carries its width, peak, mean and integral.
//! - **timeout**: the signal stays above (high) or below (low) `level` for
//!   `time`. Fires at that moment; the event carries the same, up to then.
//!
//! Every firing is an event (a log of up to [`MAX_EVENTS`]); triggers closer
//! than `holdoff` to the previous one are dropped. Crossing times are
//! interpolated between samples, and the moving average's delay is taken
//! out.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MAX_EVENTS: usize = 100_000;
const RECENT: usize = 16;
/// moving average: at most this many samples
const MAX_FILTER: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    I,
    V,
    P,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slope {
    Rise,
    Fall,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    Enter,
    Exit,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidthCond {
    Any,
    Gt,
    Lt,
    Within,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Edge { slope: Slope, level: f64 },
    Window { low: f64, high: f64, when: When },
    Pulse { polarity: Polarity, level: f64, cond: WidthCond, min: f64, max: f64 },
    Timeout { polarity: Polarity, level: f64, time: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerCfg {
    pub enabled: bool,
    pub source: Source,
    pub kind: Kind,
    /// same unit as the source, ≥ 0
    pub hysteresis: f64,
    /// s
    pub holdoff: f64,
    /// moving average length, s (0: none)
    #[serde(default)]
    pub filter: f64,
}

impl Default for TriggerCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            source: Source::I,
            kind: Kind::Edge { slope: Slope::Rise, level: 0.01 },
            hysteresis: 0.0,
            holdoff: 0.0,
            filter: 0.0,
        }
    }
}

/// What fired. Width / peak / mean / integral describe the pulse, the time
/// in or out of the window, or the time up to a timeout; NaN for an edge.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Event {
    /// number since the trigger was set (never reused)
    pub seq: u64,
    /// s: the trigger point
    pub t: f64,
    /// 'e' edge (rise), 'f' edge (fall), 'n' enter, 'x' exit, 'p' pulse, 't' timeout
    pub what: char,
    /// s
    pub width: f64,
    /// the extreme of the source over the event (max for high, min for low)
    pub peak: f64,
    pub mean: f64,
    /// ∫ source dt over the event (C for I, J for P)
    pub integral: f64,
}

/// What the UI needs every Meta: how many fired, the latest few times.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TriggerInfo {
    /// events since the trigger was set (the next event's seq)
    pub count: u64,
    /// s, oldest first
    pub recent: Vec<f64>,
}

/// A two-state comparator with hysteresis: "active" above `level` (high
/// polarity) until it falls below `level - h`; mirrored for low.
#[derive(Default)]
struct Schmitt {
    known: bool,
    active: bool,
}

impl Schmitt {
    /// Some(new state) on a change
    #[inline]
    fn update(&mut self, x: f64, level: f64, h: f64, pol: Polarity) -> Option<bool> {
        let (on, off) = match pol {
            Polarity::High => (x >= level, x < level - h || (h == 0.0 && x < level)),
            Polarity::Low => (x <= level, x > level + h || (h == 0.0 && x > level)),
        };
        if !self.known {
            self.known = true;
            self.active = on;
            return None;
        }
        if !self.active && on {
            self.active = true;
            Some(true)
        } else if self.active && off {
            self.active = false;
            Some(false)
        } else {
            None
        }
    }
}

/// Running statistics of the event being timed.
#[derive(Default, Clone, Copy)]
struct Span {
    t0: f64,
    n: u64,
    sum: f64,
    peak: f64,
}

struct Filter {
    n: usize,
    ring: Vec<f64>,
    pos: usize,
    filled: usize,
    sum: f64,
}

impl Filter {
    fn new(n: usize) -> Self {
        Self { n, ring: vec![0.0; n], pos: 0, filled: 0, sum: 0.0 }
    }

    #[inline]
    fn push(&mut self, x: f64) -> f64 {
        self.sum += x - self.ring[self.pos];
        self.ring[self.pos] = x;
        self.pos = (self.pos + 1) % self.n;
        self.filled = (self.filled + 1).min(self.n);
        // re-add now and then: float drift
        if self.pos == 0 {
            self.sum = self.ring.iter().sum();
        }
        self.sum / self.filled as f64
    }
}

pub struct Trigger {
    cfg: TriggerCfg,
    filter: Option<Filter>,
    /// filter delay to take off event times, s (set at the first sample)
    delay: f64,
    dt: f64,
    // edge
    armed_rise: bool,
    armed_fall: bool,
    // pulse / timeout / window
    sch: Schmitt,
    span: Option<Span>,
    timed_out: bool,
    prev: Option<(f64, f64)>,
    last_fire: f64,
    seq: u64,
    events: VecDeque<Event>,
    recent: VecDeque<f64>,
}

impl Default for Trigger {
    fn default() -> Self {
        Self::new(TriggerCfg::default())
    }
}

/// Time where the line from (t0, x0) to (t1, x1) crosses `level`.
#[inline]
fn cross(t0: f64, x0: f64, t1: f64, x1: f64, level: f64) -> f64 {
    if x1 == x0 {
        t1
    } else {
        (t0 + (level - x0) / (x1 - x0) * (t1 - t0)).clamp(t0, t1)
    }
}

impl Trigger {
    pub fn new(cfg: TriggerCfg) -> Self {
        Self {
            cfg,
            filter: None,
            delay: 0.0,
            dt: 0.0,
            armed_rise: false,
            armed_fall: false,
            sch: Schmitt::default(),
            span: None,
            timed_out: false,
            prev: None,
            last_fire: f64::NEG_INFINITY,
            seq: 0,
            events: VecDeque::new(),
            recent: VecDeque::new(),
        }
    }

    pub fn cfg(&self) -> &TriggerCfg {
        &self.cfg
    }

    /// New settings: starts over (events too).
    pub fn set(&mut self, cfg: TriggerCfg) {
        *self = Self::new(cfg);
    }

    /// A new stream: same settings, nothing kept.
    pub fn reset(&mut self) {
        let cfg = self.cfg.clone();
        self.set(cfg);
    }

    pub fn clear_events(&mut self) {
        self.events.clear();
        self.recent.clear();
    }

    /// Samples are missing before the next one: no crossing across the gap.
    pub fn gap(&mut self) {
        self.prev = None;
        self.span = None;
        self.sch = Schmitt::default();
        if let Some(f) = &mut self.filter {
            *f = Filter::new(f.n);
        }
    }

    #[inline]
    pub fn value(&self, i: f64, v: f64) -> f64 {
        match self.cfg.source {
            Source::I => i,
            Source::V => v,
            Source::P => i * v,
        }
    }

    #[inline]
    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    fn fire(&mut self, t: f64, what: char, span: Option<(Span, f64)>) {
        if t - self.last_fire < self.cfg.holdoff {
            return;
        }
        self.last_fire = t;
        let (width, peak, mean, integral) = match span {
            Some((s, t_end)) if s.n > 0 => {
                let mean = s.sum / s.n as f64;
                (t_end - s.t0, s.peak, mean, s.sum * self.dt)
            }
            _ => (f64::NAN, f64::NAN, f64::NAN, f64::NAN),
        };
        let e = Event { seq: self.seq, t, what, width, peak, mean, integral };
        self.seq += 1;
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(e);
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(t);
    }

    /// One sample: time t (s), source value x, `dt` s to the next one.
    #[inline]
    pub fn feed(&mut self, t: f64, x: f64, dt: f64) {
        if self.dt != dt {
            self.dt = dt;
            let n = if self.cfg.filter > 0.0 { ((self.cfg.filter / dt).round() as usize).clamp(1, MAX_FILTER) } else { 1 };
            self.filter = (n > 1).then(|| Filter::new(n));
            self.delay = (n as f64 - 1.0) / 2.0 * dt;
        }
        let x = match &mut self.filter {
            Some(f) => f.push(x),
            None => x,
        };
        // event times: where the (unfiltered) signal was
        let delay = self.delay;
        let te = t - delay;
        let h = self.cfg.hysteresis.max(0.0);
        let prev = self.prev;
        let at = move |level: f64| match prev {
            Some((tp, xp)) => cross(tp - delay, xp, te, x, level),
            None => te,
        };
        match self.cfg.kind.clone() {
            Kind::Edge { slope, level } => {
                if slope != Slope::Fall {
                    if x <= level - h || (h == 0.0 && x < level) {
                        self.armed_rise = true;
                    } else if self.armed_rise && x >= level {
                        self.armed_rise = false;
                        let tc = at(level);
                        self.fire(tc, 'e', None);
                    }
                }
                if slope != Slope::Rise {
                    if x >= level + h || (h == 0.0 && x > level) {
                        self.armed_fall = true;
                    } else if self.armed_fall && x <= level {
                        self.armed_fall = false;
                        let tc = at(level);
                        self.fire(tc, 'f', None);
                    }
                }
            }
            Kind::Window { low, high, when } => {
                // inside: low ≤ x ≤ high; leaving takes h beyond the band
                let inside = x >= low && x <= high;
                let outside = x < low - h || x > high + h || (h == 0.0 && !inside);
                if !self.sch.known {
                    self.sch.known = true;
                    self.sch.active = inside;
                    if inside {
                        self.span = Some(Span { t0: te, ..Default::default() });
                    }
                } else if !self.sch.active && inside {
                    self.sch.active = true;
                    let edge = if prev.is_some_and(|p| p.1 < low) { low } else { high };
                    let tc = at(edge);
                    if when != When::Exit {
                        self.fire(tc, 'n', None);
                    }
                    self.span = Some(Span { t0: tc, ..Default::default() });
                } else if self.sch.active && outside {
                    self.sch.active = false;
                    let edge = if x < low { low } else { high };
                    let tc = at(edge);
                    let s = self.span.take();
                    if when != When::Enter {
                        self.fire(tc, 'x', s.map(|s| (s, tc)));
                    }
                }
                if self.sch.active {
                    if let Some(s) = &mut self.span {
                        s.n += 1;
                        s.sum += x;
                        s.peak = if s.n == 1 { x } else { s.peak.max(x) };
                    }
                }
            }
            Kind::Pulse { polarity, level, cond, min, max } => {
                match self.sch.update(x, level, h, polarity) {
                    Some(true) => self.span = Some(Span { t0: at(level), ..Default::default() }),
                    Some(false) => {
                        let off = match polarity {
                            Polarity::High => level - h,
                            Polarity::Low => level + h,
                        };
                        let tc = at(if h == 0.0 { level } else { off });
                        if let Some(s) = self.span.take() {
                            let w = tc - s.t0;
                            let ok = match cond {
                                WidthCond::Any => true,
                                WidthCond::Gt => w > min,
                                WidthCond::Lt => w < max,
                                WidthCond::Within => w >= min && w <= max,
                            };
                            if ok {
                                self.fire(tc, 'p', Some((s, tc)));
                            }
                        }
                    }
                    None => {}
                }
                self.accumulate(x, polarity);
            }
            Kind::Timeout { polarity, level, time } => {
                match self.sch.update(x, level, h, polarity) {
                    Some(true) => {
                        self.span = Some(Span { t0: at(level), ..Default::default() });
                        self.timed_out = false;
                    }
                    Some(false) => {
                        self.span = None;
                        self.timed_out = false;
                    }
                    None => {}
                }
                self.accumulate(x, polarity);
                if let Some(s) = self.span {
                    if !self.timed_out && te - s.t0 >= time {
                        self.timed_out = true;
                        let tc = s.t0 + time;
                        self.fire(tc, 't', Some((s, tc)));
                    }
                }
            }
        }
        self.prev = Some((t, x));
    }

    #[inline]
    fn accumulate(&mut self, x: f64, pol: Polarity) {
        if let Some(s) = &mut self.span {
            s.n += 1;
            s.sum += x;
            s.peak = if s.n == 1 {
                x
            } else {
                match pol {
                    Polarity::High => s.peak.max(x),
                    Polarity::Low => s.peak.min(x),
                }
            };
        }
    }

    pub fn info(&self) -> TriggerInfo {
        TriggerInfo { count: self.seq, recent: self.recent.iter().copied().collect() }
    }

    /// Events with seq ≥ `since`, at most `max`, oldest first.
    pub fn events_since(&self, since: u64, max: usize) -> Vec<Event> {
        let first = self.events.front().map_or(self.seq, |e| e.seq);
        let skip = since.saturating_sub(first) as usize;
        self.events.iter().skip(skip).take(max).copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cfg: TriggerCfg, xs: &[f64]) -> Vec<Event> {
        let mut t = Trigger::new(TriggerCfg { enabled: true, ..cfg });
        for (k, &x) in xs.iter().enumerate() {
            t.feed(k as f64, x, 1.0);
        }
        t.events_since(0, usize::MAX)
    }

    fn edge(slope: Slope, level: f64) -> TriggerCfg {
        TriggerCfg { kind: Kind::Edge { slope, level }, ..Default::default() }
    }

    fn times(e: &[Event]) -> Vec<f64> {
        e.iter().map(|e| e.t).collect()
    }

    #[test]
    fn rising_with_interpolation() {
        let r = run(edge(Slope::Rise, 1.0), &[0.0, 0.5, 2.0, 0.0, 3.0]);
        assert_eq!(r.len(), 2);
        assert!((r[0].t - (1.0 + 0.5 / 1.5)).abs() < 1e-12);
        assert!((r[1].t - (3.0 + 1.0 / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn falling_and_both() {
        let xs = [0.0, 2.0, 0.0, 2.0];
        assert_eq!(times(&run(edge(Slope::Fall, 1.0), &xs)), vec![1.5]);
        assert_eq!(times(&run(edge(Slope::Both, 1.0), &xs)), vec![0.5, 1.5, 2.5]);
    }

    #[test]
    fn hysteresis_and_holdoff() {
        let xs = [0.0, 1.1, 0.9, 1.1, 0.9, 1.1, 0.0, 1.1];
        assert_eq!(run(edge(Slope::Rise, 1.0), &xs).len(), 4);
        assert_eq!(run(TriggerCfg { hysteresis: 0.5, ..edge(Slope::Rise, 1.0) }, &xs).len(), 2);
        let ys = [0.0, 2.0, 0.0, 2.0, 0.0, 2.0, 0.0, 2.0];
        assert_eq!(run(TriggerCfg { holdoff: 3.0, ..edge(Slope::Rise, 1.0) }, &ys).len(), 2);
    }

    #[test]
    fn pulse_width_conditions() {
        // pulses of 2 and 5 samples above 1.0 (crossings at half samples)
        let xs = [0.0, 2.0, 2.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0, 2.0, 0.0];
        let p = |cond, min, max| TriggerCfg {
            kind: Kind::Pulse { polarity: Polarity::High, level: 1.0, cond, min, max },
            ..Default::default()
        };
        let any = run(p(WidthCond::Any, 0.0, 0.0), &xs);
        assert_eq!(any.len(), 2);
        assert!((any[0].width - 2.0).abs() < 1e-12);
        assert!((any[1].width - 5.0).abs() < 1e-12);
        assert_eq!(any[1].peak, 2.0);
        assert_eq!(any[1].mean, 2.0);
        assert!((any[1].integral - 10.0).abs() < 1e-12); // 5 samples × 2 × dt 1
        assert!((any[0].t - 2.5).abs() < 1e-12); // fires at the end
        assert_eq!(run(p(WidthCond::Gt, 3.0, 0.0), &xs).len(), 1);
        assert_eq!(run(p(WidthCond::Lt, 0.0, 3.0), &xs).len(), 1);
        assert_eq!(run(p(WidthCond::Within, 4.0, 6.0), &xs).len(), 1);
    }

    #[test]
    fn window_enter_exit() {
        let xs = [0.0, 1.5, 1.5, 3.0, 1.5, 0.0];
        let w = |when| TriggerCfg { kind: Kind::Window { low: 1.0, high: 2.0, when }, ..Default::default() };
        let both = run(w(When::Both), &xs);
        let what: String = both.iter().map(|e| e.what).collect();
        assert_eq!(what, "nxnx");
        assert_eq!(run(w(When::Enter), &xs).len(), 2);
    }

    #[test]
    fn timeout_fires_once_per_stay() {
        let mut xs = vec![0.0; 3];
        xs.extend([2.0; 10]);
        xs.extend([0.0; 3]);
        xs.extend([2.0; 2]);
        let cfg = TriggerCfg { kind: Kind::Timeout { polarity: Polarity::High, level: 1.0, time: 4.0 }, ..Default::default() };
        let e = run(cfg, &xs);
        assert_eq!(e.len(), 1);
        assert!((e[0].t - (2.5 + 4.0)).abs() < 1e-12);
    }

    #[test]
    fn moving_average_filter_delay() {
        // a step at 100; a 21-sample average crosses 0.5 at 100 + 10 - 0.5ish, minus the delay (10)
        let mut xs = vec![0.0; 100];
        xs.extend(vec![1.0; 100]);
        let cfg = TriggerCfg { filter: 21.0, ..edge(Slope::Rise, 0.5) };
        let e = run(cfg, &xs);
        assert_eq!(e.len(), 1);
        assert!((e[0].t - 99.5).abs() < 1.0, "t = {}", e[0].t);
    }

    #[test]
    fn events_since_paging() {
        let xs: Vec<f64> = (0..100).map(|k| (k % 2) as f64 * 2.0).collect();
        let mut t = Trigger::new(TriggerCfg { enabled: true, ..edge(Slope::Rise, 1.0) });
        for (k, &x) in xs.iter().enumerate() {
            t.feed(k as f64, x, 1.0);
        }
        assert_eq!(t.info().count, 50);
        let page = t.events_since(45, 10);
        assert_eq!(page.len(), 5);
        assert_eq!(page[0].seq, 45);
    }
}

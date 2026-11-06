//! Trigger and event detection on the full-rate samples.
//!
//! The source (I, V, P or the math channel), optionally smoothed by a moving
//! average, goes through one of these detectors:
//!
//! - **edge**: crosses `level` going up / down / either. Re-arms only after
//!   the signal has been `hysteresis` past the level on the other side.
//! - **window**: enters and / or exits the band [low, high] (leaving takes
//!   `hysteresis` beyond the band).
//! - **pulse**: a pulse above (polarity high) or below (low) `level`, whose
//!   width, charge (∫ source dt), peak or mean is greater than / less than /
//!   within [min, max] / any. Fires when the pulse ends; the event carries
//!   its width, peak, mean and integral.
//! - **timeout**: the signal stays above (high) or below (low) `level` for
//!   `time`. Fires at that moment; the event carries the same, up to then.
//! - **runt**: a pulse that crosses `low` but turns back before reaching
//!   `high` (polarity high; mirrored for low). Fires as it ends.
//! - **slew**: the time to go from one level to the other (rise: low → high,
//!   fall: high → low), compared with [min, max]. Fires at the second level.
//! - **interval**: the time from one edge through `level` to the next one,
//!   compared with [min, max]. Fires at the second edge.
//! - **script**: nothing here: the samples go out in blocks
//!   ([`encode_block`]) to the user's own detector (the desktop app runs a
//!   Python function on them), whose events come back through
//!   [`Trigger::inject`] (holdoff and every Nth still apply).
//!
//! On top of the detector (in this order):
//!
//! - **sequence** (`seq`): the detector above is stage A; an event of A arms
//!   stage B (its own source and detector), which must fire at least `delay`
//!   and at most `within` after it. The B event is the trigger.
//! - **qualifier** (`qual`): only while another signal is above / below a level.
//! - **holdoff**: triggers closer than this to the previous one are dropped.
//! - **every**: only every Nth of what's left.
//!
//! Every trigger is an event (a log of up to [`MAX_EVENTS`]). Crossing times
//! are interpolated between samples, and the moving average's delay is
//! taken out.

use serde::{Deserialize, Deserializer, Serialize};
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
    /// the math channel
    M,
}

impl Source {
    #[inline]
    fn idx(self) -> usize {
        match self {
            Source::I => 0,
            Source::V => 1,
            Source::P => 2,
            Source::M => 3,
        }
    }
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

impl WidthCond {
    #[inline]
    fn ok(self, x: f64, min: f64, max: f64) -> bool {
        match self {
            WidthCond::Any => true,
            WidthCond::Gt => x > min,
            WidthCond::Lt => x < max,
            WidthCond::Within => x >= min && x <= max,
        }
    }
}

/// What a pulse's condition looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Measure {
    /// s
    #[default]
    Width,
    /// ∫ source dt: C for I, J for P
    Charge,
    /// the extreme (max above, min below)
    Peak,
    Mean,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Edge { slope: Slope, level: f64 },
    Window { low: f64, high: f64, when: When },
    Pulse {
        polarity: Polarity,
        level: f64,
        cond: WidthCond,
        min: f64,
        max: f64,
        #[serde(default)]
        measure: Measure,
    },
    Timeout { polarity: Polarity, level: f64, time: f64 },
    Runt { polarity: Polarity, low: f64, high: f64 },
    Slew { slope: Slope, low: f64, high: f64, cond: WidthCond, min: f64, max: f64 },
    Interval { slope: Slope, level: f64, cond: WidthCond, min: f64, max: f64 },
    /// detected outside (a script): events come in through `Trigger::inject`
    Script,
}

/// Only while `source` is above (high) / below (low) `level`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Qualifier {
    pub source: Source,
    pub polarity: Polarity,
    pub level: f64,
}

/// Stage B of a sequence: armed by stage A, fires the trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub source: Source,
    pub kind: Kind,
    /// s after A before B counts
    #[serde(default)]
    pub delay: f64,
    /// s after A that B may come; 0: any time (until the next A)
    #[serde(default)]
    pub within: f64,
}

fn one() -> u32 {
    1
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
    /// only every Nth trigger (1: all)
    #[serde(default = "one")]
    pub every: u32,
    #[serde(default)]
    pub qual: Option<Qualifier>,
    /// A then B: this is B
    #[serde(default)]
    pub seq: Option<Stage>,
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
            every: 1,
            qual: None,
            seq: None,
        }
    }
}

/// null (JSON has no NaN) as NaN
fn nan_or<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
}

fn nan() -> f64 {
    f64::NAN
}

/// What fired. Width / peak / mean / integral describe the pulse, the time
/// in or out of the window, or the time up to a timeout; the slew or
/// interval time is the width. NaN where they don't apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// number since the trigger was set (never reused); a candidate's own number while it waits
    pub seq: u64,
    /// s: the trigger point
    pub t: f64,
    /// 'e' edge (rise), 'f' edge (fall), 'n' enter, 'x' exit, 'p' pulse, 't' timeout,
    /// 'r' runt, 'u' rise time, 'd' fall time, 'i' interval, 'c' custom (a script)
    pub what: char,
    /// s
    #[serde(deserialize_with = "nan_or")]
    pub width: f64,
    /// the extreme of the source over the event (max for high, min for low)
    #[serde(deserialize_with = "nan_or")]
    pub peak: f64,
    #[serde(deserialize_with = "nan_or")]
    pub mean: f64,
    /// ∫ source dt over the event (C for I, J for P)
    #[serde(deserialize_with = "nan_or")]
    pub integral: f64,
    /// s: stage A's event, in a sequence
    #[serde(default = "nan", deserialize_with = "nan_or")]
    pub from: f64,
    /// the script's label for it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
#[derive(Default, Clone, Copy)]
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

impl Span {
    fn at(t0: f64) -> Self {
        Self { t0, ..Default::default() }
    }

    #[inline]
    fn add(&mut self, x: f64, pol: Polarity) {
        self.n += 1;
        self.sum += x;
        self.peak = if self.n == 1 {
            x
        } else {
            match pol {
                Polarity::High => self.peak.max(x),
                Polarity::Low => self.peak.min(x),
            }
        };
    }
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

/// A detector's firing, before the stages on top.
#[derive(Debug, Clone, Copy)]
struct Hit {
    t: f64,
    what: char,
    width: f64,
    peak: f64,
    mean: f64,
    integral: f64,
}

impl Hit {
    fn edge(t: f64, what: char) -> Self {
        Self { t, what, width: f64::NAN, peak: f64::NAN, mean: f64::NAN, integral: f64::NAN }
    }

    fn timed(t: f64, what: char, width: f64) -> Self {
        Self { width, ..Self::edge(t, what) }
    }

    fn span(t: f64, what: char, s: &Span, dt: f64) -> Self {
        if s.n == 0 {
            return Self::timed(t, what, t - s.t0);
        }
        Self { t, what, width: t - s.t0, peak: s.peak, mean: s.sum / s.n as f64, integral: s.sum * dt }
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

/// One detector on one signal: its kind's state machine.
struct Detector {
    kind: Kind,
    h: f64,
    /// moving average length, s
    filter_s: f64,
    filter: Option<Filter>,
    /// filter delay to take off event times, s (set at the first sample)
    delay: f64,
    dt: f64,
    // edge / interval
    armed_rise: bool,
    armed_fall: bool,
    // pulse / timeout / window / runt
    sch: Schmitt,
    span: Option<Span>,
    timed_out: bool,
    /// runt: the pulse reached the far level (so it isn't one)
    reached: bool,
    /// slew: crossing the first level going up / down; interval: the previous edge
    t_up: Option<f64>,
    t_down: Option<f64>,
    prev: Option<(f64, f64)>,
}

impl Detector {
    fn new(kind: Kind, h: f64, filter_s: f64) -> Self {
        Self {
            kind,
            h: h.max(0.0),
            filter_s,
            filter: None,
            delay: 0.0,
            dt: 0.0,
            armed_rise: false,
            armed_fall: false,
            sch: Schmitt::default(),
            span: None,
            timed_out: false,
            reached: false,
            t_up: None,
            t_down: None,
            prev: None,
        }
    }

    /// Samples are missing before the next one: no crossing across the gap.
    fn gap(&mut self) {
        self.prev = None;
        self.span = None;
        self.sch = Schmitt::default();
        self.t_up = None;
        self.t_down = None;
        if let Some(f) = &mut self.filter {
            *f = Filter::new(f.n);
        }
    }

    /// One sample: time t (s), value x, `dt` s to the next one; firings into `out`.
    #[inline]
    fn feed(&mut self, t: f64, x: f64, dt: f64, out: &mut Vec<Hit>) {
        if self.dt != dt {
            self.dt = dt;
            let n = if self.filter_s > 0.0 { ((self.filter_s / dt).round() as usize).clamp(1, MAX_FILTER) } else { 1 };
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
        let h = self.h;
        let prev = self.prev;
        let at = move |level: f64| match prev {
            Some((tp, xp)) => cross(tp - delay, xp, te, x, level),
            None => te,
        };
        match self.kind {
            Kind::Edge { slope, level } => {
                if let Some(tc) = self.edge_rise(slope, x, level, h, at) {
                    out.push(Hit::edge(tc, 'e'));
                }
                if let Some(tc) = self.edge_fall(slope, x, level, h, at) {
                    out.push(Hit::edge(tc, 'f'));
                }
            }
            Kind::Interval { slope, level, cond, min, max } => {
                let r = self.edge_rise(slope, x, level, h, at);
                let f = self.edge_fall(slope, x, level, h, at);
                for tc in [r, f].into_iter().flatten() {
                    if let Some(tl) = self.t_up.replace(tc) {
                        let w = tc - tl;
                        if cond.ok(w, min, max) {
                            out.push(Hit::timed(tc, 'i', w));
                        }
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
                        self.span = Some(Span::at(te));
                    }
                } else if !self.sch.active && inside {
                    self.sch.active = true;
                    let edge = if prev.is_some_and(|p| p.1 < low) { low } else { high };
                    let tc = at(edge);
                    if when != When::Exit {
                        out.push(Hit::edge(tc, 'n'));
                    }
                    self.span = Some(Span::at(tc));
                } else if self.sch.active && outside {
                    self.sch.active = false;
                    let edge = if x < low { low } else { high };
                    let tc = at(edge);
                    if let Some(s) = self.span.take() {
                        if when != When::Enter {
                            out.push(Hit::span(tc, 'x', &s, self.dt));
                        }
                    }
                }
                if self.sch.active {
                    if let Some(s) = &mut self.span {
                        s.add(x, Polarity::High);
                    }
                }
            }
            Kind::Pulse { polarity, level, cond, min, max, measure } => {
                match self.sch.update(x, level, h, polarity) {
                    Some(true) => self.span = Some(Span::at(at(level))),
                    Some(false) => {
                        let tc = at(off_level(level, h, polarity));
                        if let Some(s) = self.span.take() {
                            let hit = Hit::span(tc, 'p', &s, self.dt);
                            let v = match measure {
                                Measure::Width => hit.width,
                                Measure::Charge => hit.integral,
                                Measure::Peak => hit.peak,
                                Measure::Mean => hit.mean,
                            };
                            if cond.ok(v, min, max) {
                                out.push(hit);
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
                        self.span = Some(Span::at(at(level)));
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
                        out.push(Hit::span(tc, 't', &s, self.dt));
                    }
                }
            }
            Kind::Runt { polarity, low, high } => {
                let (lo, hi) = (low.min(high), low.max(high));
                // high: starts over `lo`, must turn back before `hi`; low: mirrored
                let (start, far) = match polarity {
                    Polarity::High => (lo, hi),
                    Polarity::Low => (hi, lo),
                };
                match self.sch.update(x, start, h, polarity) {
                    Some(true) => {
                        self.span = Some(Span::at(at(start)));
                        self.reached = false;
                    }
                    Some(false) => {
                        let tc = at(off_level(start, h, polarity));
                        if let Some(s) = self.span.take() {
                            if !self.reached {
                                out.push(Hit::span(tc, 'r', &s, self.dt));
                            }
                        }
                    }
                    None => {}
                }
                if self.sch.active {
                    let past = match polarity {
                        Polarity::High => x >= far,
                        Polarity::Low => x <= far,
                    };
                    self.reached |= past;
                }
                self.accumulate(x, polarity);
            }
            Kind::Script => {}
            Kind::Slew { slope, low, high, cond, min, max } => {
                let (lo, hi) = (low.min(high), low.max(high));
                if let Some((_, xp)) = prev {
                    if slope != Slope::Fall {
                        if x < lo {
                            self.t_up = None;
                        } else {
                            if xp < lo {
                                self.t_up = Some(at(lo));
                            }
                            if xp < hi && x >= hi {
                                if let Some(t0) = self.t_up.take() {
                                    let tc = at(hi);
                                    if cond.ok(tc - t0, min, max) {
                                        out.push(Hit::timed(tc, 'u', tc - t0));
                                    }
                                }
                            }
                        }
                    }
                    if slope != Slope::Rise {
                        if x > hi {
                            self.t_down = None;
                        } else {
                            if xp > hi {
                                self.t_down = Some(at(hi));
                            }
                            if xp > lo && x <= lo {
                                if let Some(t0) = self.t_down.take() {
                                    let tc = at(lo);
                                    if cond.ok(tc - t0, min, max) {
                                        out.push(Hit::timed(tc, 'd', tc - t0));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.prev = Some((t, x));
    }

    /// a rising edge through `level` (re-armed below it, past the hysteresis)
    #[inline]
    fn edge_rise(&mut self, slope: Slope, x: f64, level: f64, h: f64, at: impl Fn(f64) -> f64) -> Option<f64> {
        if slope == Slope::Fall {
            return None;
        }
        if x <= level - h || (h == 0.0 && x < level) {
            self.armed_rise = true;
        } else if self.armed_rise && x >= level {
            self.armed_rise = false;
            return Some(at(level));
        }
        None
    }

    #[inline]
    fn edge_fall(&mut self, slope: Slope, x: f64, level: f64, h: f64, at: impl Fn(f64) -> f64) -> Option<f64> {
        if slope == Slope::Rise {
            return None;
        }
        if x >= level + h || (h == 0.0 && x > level) {
            self.armed_fall = true;
        } else if self.armed_fall && x <= level {
            self.armed_fall = false;
            return Some(at(level));
        }
        None
    }

    #[inline]
    fn accumulate(&mut self, x: f64, pol: Polarity) {
        if let Some(s) = &mut self.span {
            s.add(x, pol);
        }
    }
}

/// where a pulse above (below) `level` ends, with hysteresis `h`
#[inline]
fn off_level(level: f64, h: f64, pol: Polarity) -> f64 {
    if h == 0.0 {
        level
    } else {
        match pol {
            Polarity::High => level - h,
            Polarity::Low => level + h,
        }
    }
}

pub struct Trigger {
    cfg: TriggerCfg,
    a: Detector,
    /// stage B of a sequence
    b: Option<Detector>,
    /// a sequence: stage A's latest event, waiting for B
    armed_at: Option<f64>,
    hits: Vec<Hit>,
    hits_b: Vec<Hit>,
    last_fire: f64,
    /// triggers since the last one kept (every Nth)
    skipped: u32,
    seq: u64,
    events: VecDeque<Event>,
    recent: VecDeque<f64>,
}

impl Default for Trigger {
    fn default() -> Self {
        Self::new(TriggerCfg::default())
    }
}

impl Trigger {
    pub fn new(cfg: TriggerCfg) -> Self {
        let a = Detector::new(cfg.kind.clone(), cfg.hysteresis, cfg.filter);
        let b = cfg.seq.as_ref().map(|s| Detector::new(s.kind.clone(), cfg.hysteresis, cfg.filter));
        Self {
            cfg,
            a,
            b,
            armed_at: None,
            hits: Vec::new(),
            hits_b: Vec::new(),
            last_fire: f64::NEG_INFINITY,
            skipped: 0,
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
        self.a.gap();
        if let Some(b) = &mut self.b {
            b.gap();
        }
        self.armed_at = None;
    }

    /// the source's value of a sample (A, V, math)
    #[inline]
    pub fn value(&self, i: f64, v: f64, m: f64) -> f64 {
        [i, v, i * v, m][self.cfg.source.idx()]
    }

    #[inline]
    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    /// One sample of a single signal (every source reads `x`): time t (s), `dt` s to the next one.
    #[inline]
    pub fn feed(&mut self, t: f64, x: f64, dt: f64) {
        self.feed_all(t, [x; 4], dt);
    }

    /// One sample: time t (s), [A, V, W, math], `dt` s to the next one.
    #[inline]
    pub fn feed_all(&mut self, t: f64, vals: [f64; 4], dt: f64) {
        if self.cfg.kind == Kind::Script {
            return;
        }
        let mut hits = std::mem::take(&mut self.hits);
        let mut hits_b = std::mem::take(&mut self.hits_b);
        self.a.feed(t, vals[self.cfg.source.idx()], dt, &mut hits);
        if let (Some(b), Some(s)) = (&mut self.b, &self.cfg.seq) {
            b.feed(t, vals[s.source.idx()], dt, &mut hits_b);
        }
        match self.cfg.seq.as_ref().map(|s| (s.delay, s.within)) {
            None => {
                for h in hits.drain(..) {
                    self.candidate(h, f64::NAN, &vals);
                }
            }
            Some((delay, within)) => {
                // A arms (again, from its latest event); B within the window fires, once per A
                if let Some(h) = hits.last() {
                    self.armed_at = Some(h.t);
                }
                hits.clear();
                for h in hits_b.drain(..) {
                    let Some(ta) = self.armed_at else { break };
                    let late = within > 0.0 && h.t - ta > within;
                    if late {
                        self.armed_at = None;
                    } else if h.t > ta && h.t - ta >= delay {
                        self.armed_at = None;
                        self.candidate(h, ta, &vals);
                    }
                }
                if let Some(ta) = self.armed_at {
                    if within > 0.0 && t - self.a.delay - ta > within {
                        self.armed_at = None;
                    }
                }
            }
        }
        self.hits = hits;
        self.hits_b = hits_b;
    }

    /// A detector's (or B's) firing: the qualifier, holdoff and every Nth, then the log.
    fn candidate(&mut self, h: Hit, from: f64, vals: &[f64; 4]) {
        if let Some(q) = &self.cfg.qual {
            let x = vals[q.source.idx()];
            let ok = match q.polarity {
                Polarity::High => x >= q.level,
                Polarity::Low => x <= q.level,
            };
            if !ok {
                return;
            }
        }
        let e = Event { seq: 0, t: h.t, what: h.what, width: h.width, peak: h.peak, mean: h.mean, integral: h.integral, from, label: None };
        self.keep(e);
    }

    /// holdoff and every Nth, then into the log
    fn keep(&mut self, e: Event) {
        if e.t - self.last_fire < self.cfg.holdoff {
            return;
        }
        self.last_fire = e.t;
        self.skipped += 1;
        if self.skipped < self.cfg.every.max(1) {
            return;
        }
        self.skipped = 0;
        self.log(e);
    }

    fn log(&mut self, mut e: Event) {
        e.seq = self.seq;
        self.seq += 1;
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(e.t);
        self.events.push_back(e);
    }

    /// Events found outside (the script trigger), in time order: holdoff and every Nth, then the log.
    pub fn inject(&mut self, mut events: Vec<Event>) {
        if !self.cfg.enabled || self.cfg.kind != Kind::Script {
            return;
        }
        events.retain(|e| e.t.is_finite());
        events.sort_by(|a, b| a.t.total_cmp(&b.t));
        for e in events {
            self.keep(Event { what: 'c', ..e });
        }
    }

    pub fn info(&self) -> TriggerInfo {
        TriggerInfo {
            count: self.seq,
            recent: self.recent.iter().copied().collect(),
        }
    }

    /// Events with seq ≥ `since`, at most `max`, oldest first.
    pub fn events_since(&self, since: u64, max: usize) -> Vec<Event> {
        let first = self.events.front().map_or(self.seq, |e| e.seq);
        let skip = since.saturating_sub(first) as usize;
        self.events.iter().skip(skip).take(max).cloned().collect()
    }
}

/// A block of raw samples for an outside detector (the script trigger), little-endian:
///
/// ```text
/// u32 n           samples
/// u32 runs        stretches without a gap
/// u32 has_math    1 if the m channel follows
/// f64 dt          s per sample
/// runs × (f64 t0, u32 count)   each stretch's first time (s) and length
/// f32 × n  i (A)   f32 × n  v (V)   [f32 × n  m]
/// ```
pub fn encode_block(dt: f64, runs: &[(f64, u32)], i: &[f32], v: &[f32], m: Option<&[f32]>) -> Vec<u8> {
    let n = i.len();
    let mut out = Vec::with_capacity(20 + runs.len() * 12 + n * if m.is_some() { 12 } else { 8 });
    out.extend((n as u32).to_le_bytes());
    out.extend((runs.len() as u32).to_le_bytes());
    out.extend((m.is_some() as u32).to_le_bytes());
    out.extend(dt.to_le_bytes());
    for &(t0, c) in runs {
        out.extend(t0.to_le_bytes());
        out.extend(c.to_le_bytes());
    }
    for a in [Some(i), Some(v), m].into_iter().flatten() {
        for x in a {
            out.extend(x.to_le_bytes());
        }
    }
    out
}

/// Bytes as base64 (standard alphabet, padded): a block to carry in JSON.
pub fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(A[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
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
            kind: Kind::Pulse { polarity: Polarity::High, level: 1.0, cond, min, max, measure: Measure::Width },
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

    fn whats(e: &[Event]) -> String {
        e.iter().map(|e| e.what).collect()
    }

    #[test]
    fn pulse_by_charge_peak_mean() {
        // pulses above 1.0: 2 samples of 2.0 (charge 4), 3 samples of 5.0 (charge 15)
        let xs = [0.0, 2.0, 2.0, 0.0, 5.0, 5.0, 5.0, 0.0];
        let p = |measure, cond, min, max| TriggerCfg {
            kind: Kind::Pulse { polarity: Polarity::High, level: 1.0, cond, min, max, measure },
            ..Default::default()
        };
        let big = run(p(Measure::Charge, WidthCond::Gt, 10.0, 0.0), &xs);
        assert_eq!(big.len(), 1);
        assert_eq!(big[0].integral, 15.0);
        assert_eq!(run(p(Measure::Charge, WidthCond::Lt, 0.0, 10.0), &xs).len(), 1);
        assert_eq!(run(p(Measure::Peak, WidthCond::Within, 4.0, 6.0), &xs)[0].peak, 5.0);
        assert_eq!(run(p(Measure::Mean, WidthCond::Lt, 0.0, 3.0), &xs)[0].mean, 2.0);
    }

    #[test]
    fn runt_turns_back_before_the_far_level() {
        // a full pulse to 3, a runt to 1.5, a full one: levels 1 / 2
        let xs = [0.0, 3.0, 0.0, 1.5, 0.0, 3.0, 0.0];
        let r = |polarity, low, high| TriggerCfg { kind: Kind::Runt { polarity, low, high }, ..Default::default() };
        let e = run(r(Polarity::High, 1.0, 2.0), &xs);
        assert_eq!(whats(&e), "r");
        assert_eq!(e[0].peak, 1.5);
        assert!(e[0].t > 3.0 && e[0].t < 4.0);
        // below: dips from 3 that turn back before 1
        let ys = [3.0, 0.0, 3.0, 1.5, 3.0];
        assert_eq!(run(r(Polarity::Low, 1.0, 2.0), &ys).len(), 1);
    }

    #[test]
    fn slew_times_between_levels() {
        // a slow rise 0 → 4 over 4 samples, a fast one in 1; levels 1 and 3: 2 s and 0.5 s
        let xs = [0.0, 1.0, 2.0, 3.0, 4.0, 0.0, 4.0, 4.0, 0.0];
        let s = |slope, cond, min, max| TriggerCfg { kind: Kind::Slew { slope, low: 1.0, high: 3.0, cond, min, max }, ..Default::default() };
        let all = run(s(Slope::Rise, WidthCond::Any, 0.0, 0.0), &xs);
        assert_eq!(all.len(), 2);
        assert!((all[0].width - 2.0).abs() < 1e-12, "{}", all[0].width);
        assert!((all[1].width - 0.5).abs() < 1e-12, "{}", all[1].width);
        assert_eq!(run(s(Slope::Rise, WidthCond::Gt, 1.0, 0.0), &xs).len(), 1);
        let falls = run(s(Slope::Fall, WidthCond::Any, 0.0, 0.0), &xs);
        assert_eq!(whats(&falls), "dd");
        // a rise that turns back between the levels doesn't count
        let ys = [0.0, 2.0, 0.0, 2.0, 4.0];
        let e = run(s(Slope::Rise, WidthCond::Any, 0.0, 0.0), &ys);
        assert_eq!(e.len(), 1);
        assert!((e[0].width - 1.0).abs() < 1e-12, "{}", e[0].width);
    }

    #[test]
    fn interval_between_edges() {
        // rising edges at 0.5, 2.5, 7.5: intervals 2 and 5
        let xs = [0.0, 2.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0];
        let iv = |cond, min, max| TriggerCfg { kind: Kind::Interval { slope: Slope::Rise, level: 1.0, cond, min, max }, ..Default::default() };
        let all = run(iv(WidthCond::Any, 0.0, 0.0), &xs);
        assert_eq!(times(&all), vec![2.5, 7.5]);
        assert_eq!(all[1].width, 5.0);
        assert_eq!(times(&run(iv(WidthCond::Gt, 3.0, 0.0), &xs)), vec![7.5]);
    }

    #[test]
    fn every_nth_and_qualifier() {
        let mut t = Trigger::new(TriggerCfg {
            enabled: true,
            every: 3,
            ..edge(Slope::Rise, 1.0)
        });
        for k in 0..20 {
            t.feed(k as f64, (k % 2) as f64 * 2.0, 1.0);
        }
        assert_eq!(t.info().count, 3); // 10 edges, every 3rd
        // I edges only while V ≥ 3: V is 4 for the first half, 2 after
        let mut q = Trigger::new(TriggerCfg {
            enabled: true,
            qual: Some(Qualifier { source: Source::V, polarity: Polarity::High, level: 3.0 }),
            ..edge(Slope::Rise, 1.0)
        });
        for k in 0..20 {
            let v = if k < 10 { 4.0 } else { 2.0 };
            q.feed_all(k as f64, [(k % 2) as f64 * 2.0, v, 0.0, 0.0], 1.0);
        }
        assert_eq!(q.info().count, 5);
    }

    #[test]
    fn sequence_a_then_b() {
        // A: I rises through 1; B: V rises through 1, between 2 and 5 s after A
        let cfg = TriggerCfg {
            enabled: true,
            seq: Some(Stage { source: Source::V, kind: Kind::Edge { slope: Slope::Rise, level: 1.0 }, delay: 2.0, within: 5.0 }),
            ..edge(Slope::Rise, 1.0)
        };
        let mut t = Trigger::new(cfg);
        // I edge at 0.5; V edges at 1.5 (too soon), 4.5 (fires), 6.5 (spent); I at 10.5, V at 20.5 (too late)
        let i_at = [1, 11];
        let v_at = [2, 5, 7, 21];
        for k in 0..30 {
            let i = if i_at.contains(&k) { 2.0 } else { 0.0 };
            let v = if v_at.contains(&k) { 2.0 } else { 0.0 };
            t.feed_all(k as f64, [i, v, 0.0, 0.0], 1.0);
        }
        let e = t.events_since(0, 10);
        assert_eq!(times(&e), vec![4.5]);
        assert_eq!(e[0].from, 0.5);
    }

    #[test]
    fn base64_encodes() {
        assert_eq!(base64(b"hello world"), "aGVsbG8gd29ybGQ=");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64(b""), "");
    }

    #[test]
    fn script_events_come_in_from_outside() {
        let mut t = Trigger::new(TriggerCfg { enabled: true, kind: Kind::Script, holdoff: 1.0, ..Default::default() });
        for k in 0..10 {
            t.feed(k as f64, (k % 2) as f64 * 2.0, 1.0); // nothing detected here
        }
        assert_eq!(t.info().count, 0);
        let ev = |t: f64, label: &str| Event { seq: 0, t, what: '?', width: f64::NAN, peak: f64::NAN, mean: f64::NAN, integral: f64::NAN, from: f64::NAN, label: Some(label.into()) };
        // out of order, one inside the holdoff of another
        t.inject(vec![ev(5.0, "b"), ev(2.0, "a"), ev(5.5, "c")]);
        let e = t.events_since(0, 10);
        assert_eq!(times(&e), vec![2.0, 5.0]);
        assert_eq!(whats(&e), "cc");
        assert_eq!(e[1].label.as_deref(), Some("b"));
        // not a script trigger: ignored
        let mut edge_t = Trigger::new(TriggerCfg { enabled: true, ..edge(Slope::Rise, 1.0) });
        edge_t.inject(vec![ev(1.0, "x")]);
        assert_eq!(edge_t.info().count, 0);
    }

    #[test]
    fn event_round_trips_through_json() {
        let e = Event { seq: 1, t: 2.0, what: 'e', width: f64::NAN, peak: 1.0, mean: f64::NAN, integral: f64::NAN, from: f64::NAN, label: None };
        let s = serde_json::to_string(&e).unwrap();
        let back: Event = serde_json::from_str(&s).unwrap();
        assert!(back.width.is_nan() && back.from.is_nan());
        assert_eq!(back.peak, 1.0);
        // an old config without the new fields still reads
        let c: TriggerCfg = serde_json::from_str(r#"{"enabled":true,"source":"i","kind":{"type":"pulse","polarity":"high","level":1,"cond":"any","min":0,"max":0},"hysteresis":0,"holdoff":0}"#).unwrap();
        assert_eq!(c.every, 1);
        assert!(c.seq.is_none());
    }
}

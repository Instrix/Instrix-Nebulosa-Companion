//! Edge trigger on the full-rate samples.
//!
//! Rising: the signal crosses `level` going up, after it has been at or
//! below `level - hysteresis` (armed). Falling: the mirror image. Either:
//! both. The hysteresis keeps noise sitting on the level from firing again
//! and again. Triggers closer than `holdoff` to the previous one are dropped.
//! The time of a trigger is interpolated between the two samples around the
//! crossing.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerCfg {
    pub enabled: bool,
    pub source: Source,
    pub slope: Slope,
    /// A, V or W
    pub level: f64,
    /// same unit, ≥ 0
    pub hysteresis: f64,
    /// s
    pub holdoff: f64,
}

impl Default for TriggerCfg {
    fn default() -> Self {
        Self { enabled: false, source: Source::I, slope: Slope::Rise, level: 0.01, hysteresis: 0.0, holdoff: 0.0 }
    }
}

/// What the UI needs: how many fired, and the latest few times.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TriggerInfo {
    pub count: u64,
    /// s, oldest first
    pub recent: Vec<f64>,
}

const RECENT: usize = 16;

pub struct Trigger {
    cfg: TriggerCfg,
    armed_rise: bool,
    armed_fall: bool,
    prev: Option<(f64, f64)>,
    last: f64,
    count: u64,
    recent: VecDeque<f64>,
}

impl Default for Trigger {
    fn default() -> Self {
        Self::new(TriggerCfg::default())
    }
}

impl Trigger {
    pub fn new(cfg: TriggerCfg) -> Self {
        Self {
            cfg,
            armed_rise: false,
            armed_fall: false,
            prev: None,
            last: f64::NEG_INFINITY,
            count: 0,
            recent: VecDeque::new(),
        }
    }

    pub fn cfg(&self) -> &TriggerCfg {
        &self.cfg
    }

    /// New settings: disarm, keep nothing from before.
    pub fn set(&mut self, cfg: TriggerCfg) {
        *self = Self::new(cfg);
    }

    /// Samples are missing before the next one: don't trigger across the gap.
    pub fn gap(&mut self) {
        self.prev = None;
    }

    pub fn reset(&mut self) {
        let cfg = self.cfg.clone();
        self.set(cfg);
    }

    /// Picks this trigger's source from a sample.
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

    /// One sample: time t (s), value x.
    #[inline]
    pub fn feed(&mut self, t: f64, x: f64) {
        let c = &self.cfg;
        let (lvl, h) = (c.level, c.hysteresis.max(0.0));
        let rise = c.slope != Slope::Fall;
        let fall = c.slope != Slope::Rise;
        let mut fired = false;
        // each direction arms on its own side of the level, then fires on the crossing
        if rise {
            if x <= lvl - h || (h == 0.0 && x < lvl) {
                self.armed_rise = true;
            } else if self.armed_rise && x >= lvl {
                self.armed_rise = false;
                fired = true;
            }
        }
        if fall {
            if x >= lvl + h || (h == 0.0 && x > lvl) {
                self.armed_fall = true;
            } else if self.armed_fall && x <= lvl {
                self.armed_fall = false;
                fired = true;
            }
        }
        if fired {
            // the crossing, between the previous sample and this one
            let tc = match self.prev {
                Some((tp, xp)) if x != xp => (tp + (lvl - xp) / (x - xp) * (t - tp)).clamp(tp, t),
                _ => t,
            };
            if tc - self.last >= self.cfg.holdoff {
                self.last = tc;
                self.count += 1;
                if self.recent.len() == RECENT {
                    self.recent.pop_front();
                }
                self.recent.push_back(tc);
            }
        }
        self.prev = Some((t, x));
    }

    pub fn info(&self) -> TriggerInfo {
        TriggerInfo { count: self.count, recent: self.recent.iter().copied().collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cfg: TriggerCfg, xs: &[f64]) -> Vec<f64> {
        let mut t = Trigger::new(TriggerCfg { enabled: true, ..cfg });
        for (k, &x) in xs.iter().enumerate() {
            t.feed(k as f64, x);
        }
        t.info().recent
    }

    #[test]
    fn rising_with_interpolation() {
        let r = run(TriggerCfg { level: 1.0, ..Default::default() }, &[0.0, 0.5, 2.0, 0.0, 3.0]);
        assert_eq!(r.len(), 2);
        assert!((r[0] - (1.0 + 0.5 / 1.5)).abs() < 1e-12); // between samples 1 and 2
        assert!((r[1] - (3.0 + 1.0 / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn falling_and_both() {
        let xs = [0.0, 2.0, 0.0, 2.0];
        let f = run(TriggerCfg { level: 1.0, slope: Slope::Fall, ..Default::default() }, &xs);
        assert_eq!(f, vec![1.5]);
        let b = run(TriggerCfg { level: 1.0, slope: Slope::Both, ..Default::default() }, &xs);
        assert_eq!(b, vec![0.5, 1.5, 2.5]);
    }

    #[test]
    fn hysteresis_rejects_noise() {
        // noise around the level: without hysteresis it fires each time
        let xs = [0.0, 1.1, 0.9, 1.1, 0.9, 1.1, 0.0, 1.1];
        assert_eq!(run(TriggerCfg { level: 1.0, ..Default::default() }, &xs).len(), 4);
        // with 0.5 it needs to come down to 0.5 first
        assert_eq!(run(TriggerCfg { level: 1.0, hysteresis: 0.5, ..Default::default() }, &xs).len(), 2);
    }

    #[test]
    fn holdoff() {
        let xs = [0.0, 2.0, 0.0, 2.0, 0.0, 2.0, 0.0, 2.0];
        assert_eq!(run(TriggerCfg { level: 1.0, holdoff: 3.0, ..Default::default() }, &xs).len(), 2);
    }
}

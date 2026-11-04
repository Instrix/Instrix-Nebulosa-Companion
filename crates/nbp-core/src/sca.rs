//! Side-channel power analysis: a campaign of traces, each the same number of
//! samples cut out of the buffer at a trigger, with what the target was given
//! and what it answered.
//!
//! - **Align**: each trace's shift (± a few samples) that best matches a
//!   reference window, by correlation; the reference is refined once with the
//!   mean of the aligned traces. Every view after it uses the shifts.
//! - **Overview**: the mean and standard deviation per sample, and a few
//!   traces to draw.
//! - **T-test**: Welch's t per sample between two groups of traces (TVLA:
//!   fixed input vs random input); |t| > 4.5 marks a leak.
//! - **Export**: an .npz (traces, inputs, outputs, groups, widths, meta.json)
//!   that numpy, ChipWhisperer's analyzer, Lascar or SCAred can load.

use crate::analysis::Extract;
use crate::trigger::Source;
use serde::{Deserialize, Serialize};

/// The most samples a campaign keeps (all traces together): 400 MB of f32.
pub const MAX_VALUES: usize = 100_000_000;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// samples per trace
    pub samples: usize,
    pub source: Source,
}

/// What the target did for one trace.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TraceMeta {
    #[serde(default)]
    pub input: Vec<u8>,
    #[serde(default)]
    pub output: Vec<u8>,
    /// TVLA: 0 fixed input, 1 random input
    #[serde(default)]
    pub group: u8,
    /// the trigger event's width, s (pulse triggers; 0 otherwise)
    #[serde(default)]
    pub width: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub n: usize,
    pub samples: usize,
    /// s per sample (0 until the first trace)
    pub dt: f64,
    pub source: Source,
    pub aligned: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    /// mean and σ per sample, and the first `show` traces
    Overview { show: usize },
    /// Welch's t between group 0 and group 1
    TTest,
    /// per-trace active duration: samples above `threshold`, in seconds (a
    /// timing attack groups these by the input outside the core)
    Timing { threshold: f64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Result {
    Overview {
        n: usize,
        dt: f64,
        mean: Vec<f32>,
        std: Vec<f32>,
        traces: Vec<Vec<f32>>,
    },
    TTest {
        dt: f64,
        t: Vec<f32>,
        n0: usize,
        n1: usize,
        /// the largest |t| and its sample
        max_abs: f32,
        at: usize,
    },
    Timing {
        dt: f64,
        /// seconds above the threshold, one per trace (aligned)
        durations: Vec<f32>,
    },
    Empty {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct AlignResult {
    /// shifts applied, samples (one per trace)
    pub shifts: Vec<i32>,
    /// mean correlation with the reference after aligning
    pub mean_corr: f32,
    /// traces whose best correlation was under 0.5 (likely a different event)
    pub poor: usize,
}

pub struct Campaign {
    cfg: Config,
    dt: f64,
    /// n × samples, row by row
    data: Vec<f32>,
    shifts: Vec<i32>,
    meta: Vec<TraceMeta>,
}

impl Campaign {
    pub fn new(cfg: Config) -> std::result::Result<Self, String> {
        if cfg.samples < 2 {
            return Err("a trace needs at least 2 samples".into());
        }
        if cfg.samples > MAX_VALUES {
            return Err("traces that long don't fit in memory".into());
        }
        Ok(Self { cfg, dt: 0.0, data: Vec::new(), shifts: Vec::new(), meta: Vec::new() })
    }

    pub fn len(&self) -> usize {
        self.meta.len()
    }

    pub fn is_empty(&self) -> bool {
        self.meta.is_empty()
    }

    pub fn info(&self) -> Info {
        Info {
            n: self.len(),
            samples: self.cfg.samples,
            dt: self.dt,
            source: self.cfg.source,
            aligned: self.shifts.iter().any(|&s| s != 0),
        }
    }

    pub fn meta(&self) -> &[TraceMeta] {
        &self.meta
    }

    /// One trace from the start of `x` (copied out of the buffer from the trace's start time).
    pub fn add(&mut self, x: &Extract, meta: TraceMeta) -> std::result::Result<usize, String> {
        let l = self.cfg.samples;
        if (self.len() + 1) * l > MAX_VALUES {
            return Err(format!("the campaign is full ({} traces)", self.len()));
        }
        let run = x.runs.first().ok_or("no samples there (not in memory any more?)")?;
        if run.2.len() < l {
            return Err(if x.runs.len() > 1 { "a gap in the samples inside the trace".into() } else { "not enough samples yet".into() });
        }
        if self.dt == 0.0 {
            self.dt = run.1;
        } else if (run.1 - self.dt).abs() > self.dt * 1e-6 {
            return Err("the sample rate changed: start a new campaign".into());
        }
        let src = self.cfg.source;
        match (src, &run.3) {
            (Source::M, Some(m)) => self.data.extend(m[..l].iter().copied()),
            (Source::M, None) => return Err("no math channel".into()),
            _ => self.data.extend(run.2[..l].iter().map(|&w| x.value(w, src) as f32)),
        }
        self.shifts.push(0);
        self.meta.push(meta);
        Ok(self.len())
    }

    fn raw(&self, k: usize) -> &[f32] {
        let l = self.cfg.samples;
        &self.data[k * l..(k + 1) * l]
    }

    /// Trace k with its shift applied (edges held at the nearest sample).
    pub fn aligned(&self, k: usize, out: &mut Vec<f32>) {
        let r = self.raw(k);
        let s = self.shifts[k];
        let l = r.len() as i64;
        out.clear();
        out.extend((0..l).map(|i| r[(i + s as i64).clamp(0, l - 1) as usize]));
    }

    pub fn clear_align(&mut self) {
        self.shifts.iter_mut().for_each(|s| *s = 0);
    }

    /// Shifts that line every trace's [a, b) up with a reference, within ±max_shift samples.
    pub fn align(&mut self, a: usize, b: usize, max_shift: usize) -> std::result::Result<AlignResult, String> {
        let l = self.cfg.samples;
        let b = b.min(l);
        if self.is_empty() {
            return Err("no traces".into());
        }
        if b <= a + 2 {
            return Err("choose a wider window to align on".into());
        }
        let m = max_shift as i64;
        // the reference: trace 0's window, then the mean of the aligned windows
        let mut reference: Vec<f64> = self.raw(0)[a..b].iter().map(|&x| x as f64).collect();
        let mut best_corr = vec![0f32; self.len()];
        for pass in 0..2 {
            for k in 0..self.len() {
                let r = self.raw(k);
                let mut best = (f64::NEG_INFINITY, 0i64);
                for s in -m..=m {
                    let lo = a as i64 + s;
                    let hi = b as i64 + s;
                    if lo < 0 || hi > l as i64 {
                        continue;
                    }
                    let c = pearson(&reference, &r[lo as usize..hi as usize]);
                    if c > best.0 {
                        best = (c, s);
                    }
                }
                self.shifts[k] = best.1 as i32;
                best_corr[k] = best.0 as f32;
            }
            if pass == 0 {
                let mut acc = vec![0f64; b - a];
                for k in 0..self.len() {
                    let r = self.raw(k);
                    let s = self.shifts[k] as i64;
                    for (j, v) in acc.iter_mut().enumerate() {
                        *v += r[(a as i64 + j as i64 + s).clamp(0, l as i64 - 1) as usize] as f64;
                    }
                }
                let n = self.len() as f64;
                reference = acc.into_iter().map(|v| v / n).collect();
            }
        }
        let finite: Vec<f32> = best_corr.iter().copied().filter(|c| c.is_finite()).collect();
        Ok(AlignResult {
            shifts: self.shifts.clone(),
            mean_corr: finite.iter().sum::<f32>() / finite.len().max(1) as f32,
            poor: best_corr.iter().filter(|&&c| !(c >= 0.5)).count(),
        })
    }

    pub fn query(&self, req: &Request) -> Result {
        if self.is_empty() {
            return Result::Empty { reason: "no traces yet".into() };
        }
        let l = self.cfg.samples;
        let mut buf = Vec::with_capacity(l);
        match *req {
            Request::Overview { show } => {
                let mut acc = vec![Welford::default(); l];
                let mut traces = Vec::new();
                for k in 0..self.len() {
                    self.aligned(k, &mut buf);
                    for (w, &x) in acc.iter_mut().zip(&buf) {
                        w.push(x as f64);
                    }
                    if k < show {
                        traces.push(buf.clone());
                    }
                }
                Result::Overview {
                    n: self.len(),
                    dt: self.dt,
                    mean: acc.iter().map(|w| w.mean as f32).collect(),
                    std: acc.iter().map(|w| w.std() as f32).collect(),
                    traces,
                }
            }
            Request::Timing { threshold } => {
                let mut durations = Vec::with_capacity(self.len());
                for k in 0..self.len() {
                    self.aligned(k, &mut buf);
                    let n = buf.iter().filter(|&&x| x as f64 > threshold).count();
                    durations.push(n as f32 * self.dt as f32);
                }
                Result::Timing { dt: self.dt, durations }
            }
            Request::TTest => {
                let mut g = [vec![Welford::default(); l], vec![Welford::default(); l]];
                for k in 0..self.len() {
                    let grp = (self.meta[k].group != 0) as usize;
                    self.aligned(k, &mut buf);
                    for (w, &x) in g[grp].iter_mut().zip(&buf) {
                        w.push(x as f64);
                    }
                }
                let (n0, n1) = (g[0][0].n as usize, g[1][0].n as usize);
                if n0 < 2 || n1 < 2 {
                    return Result::Empty { reason: "the t-test needs at least 2 traces in each group (fixed and random inputs)".into() };
                }
                let t: Vec<f32> = g[0]
                    .iter()
                    .zip(&g[1])
                    .map(|(a, b)| {
                        let d = (a.var() / a.n + b.var() / b.n).sqrt();
                        if d > 0.0 { ((a.mean - b.mean) / d) as f32 } else { 0.0 }
                    })
                    .collect();
                let (at, max_abs) = t.iter().enumerate().fold((0, 0f32), |m, (i, &v)| if v.abs() > m.1 { (i, v.abs()) } else { m });
                Result::TTest { dt: self.dt, t, n0, n1, max_abs, at }
            }
        }
    }

    /// The campaign as an .npz (stored, no compression): traces.npy (aligned,
    /// float32 n × samples), textin.npy / textout.npy (uint8 n × the longest,
    /// zero-padded), group.npy (uint8), width.npy (float64), meta.json.
    pub fn export_npz(&self) -> Vec<u8> {
        let n = self.len();
        let l = self.cfg.samples;
        let mut traces = Vec::with_capacity(n * l * 4);
        let mut buf = Vec::with_capacity(l);
        for k in 0..n {
            self.aligned(k, &mut buf);
            for x in &buf {
                traces.extend_from_slice(&x.to_le_bytes());
            }
        }
        let bytes = |f: &dyn Fn(&TraceMeta) -> &Vec<u8>| {
            let w = self.meta.iter().map(|m| f(m).len()).max().unwrap_or(0);
            let mut v = vec![0u8; n * w];
            for (k, m) in self.meta.iter().enumerate() {
                v[k * w..k * w + f(m).len()].copy_from_slice(f(m));
            }
            (v, w)
        };
        let (tin, win) = bytes(&|m| &m.input);
        let (tout, wout) = bytes(&|m| &m.output);
        let group: Vec<u8> = self.meta.iter().map(|m| m.group).collect();
        let width: Vec<u8> = self.meta.iter().flat_map(|m| m.width.to_le_bytes()).collect();
        let meta = serde_json::json!({
            "app": "Nebulosa Companion",
            "samples": l,
            "traces": n,
            "dt": self.dt,
            "sample_rate": if self.dt > 0.0 { 1.0 / self.dt } else { 0.0 },
            "source": self.cfg.source,
            "unit": match self.cfg.source { Source::I => "A", Source::V => "V", Source::P => "W", Source::M => "" },
            "shifts": self.shifts,
        });
        let mut z = Zip::default();
        z.add("traces.npy", &npy("<f4", &[n, l], &traces));
        z.add("textin.npy", &npy("|u1", &[n, win], &tin));
        z.add("textout.npy", &npy("|u1", &[n, wout], &tout));
        z.add("group.npy", &npy("|u1", &[n], &group));
        z.add("width.npy", &npy("<f8", &[n], &width));
        z.add("meta.json", serde_json::to_string_pretty(&meta).unwrap_or_default().as_bytes());
        z.finish()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Welford {
    n: f64,
    mean: f64,
    m2: f64,
}

impl Welford {
    fn push(&mut self, x: f64) {
        self.n += 1.0;
        let d = x - self.mean;
        self.mean += d / self.n;
        self.m2 += d * (x - self.mean);
    }
    /// sample variance
    fn var(&self) -> f64 {
        if self.n > 1.0 { self.m2 / (self.n - 1.0) } else { 0.0 }
    }
    fn std(&self) -> f64 {
        self.var().sqrt()
    }
}

fn pearson(a: &[f64], b: &[f32]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().map(|&x| x as f64).sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let (dx, dy) = (x - ma, y as f64 - mb);
        sab += dx * dy;
        saa += dx * dx;
        sbb += dy * dy;
    }
    if saa > 0.0 && sbb > 0.0 { sab / (saa * sbb).sqrt() } else { f64::NEG_INFINITY }
}

/// A .npy (format 1.0): the header padded to a multiple of 64 bytes.
fn npy(descr: &str, shape: &[usize], data: &[u8]) -> Vec<u8> {
    let dims: Vec<String> = shape.iter().map(|d| d.to_string()).collect();
    let shape = if dims.len() == 1 { format!("({},)", dims[0]) } else { format!("({})", dims.join(", ")) };
    let mut h = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {shape}, }}");
    let pad = (64 - (10 + h.len() + 1) % 64) % 64;
    h.push_str(&" ".repeat(pad));
    h.push('\n');
    let mut out = Vec::with_capacity(10 + h.len() + data.len());
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    out.extend_from_slice(&(h.len() as u16).to_le_bytes());
    out.extend_from_slice(h.as_bytes());
    out.extend_from_slice(data);
    out
}

/// A zip with stored (uncompressed) entries: what numpy's .npz reader takes.
#[derive(Default)]
struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Zip {
    fn add(&mut self, name: &str, data: &[u8]) {
        let crc = crate::update::crc32(data);
        let off = self.out.len() as u32;
        let len = data.len() as u32;
        let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        // local header
        le32(&mut self.out, 0x0403_4b50);
        for x in [20u16, 0, 0, 0, 0x21] {
            le16(&mut self.out, x); // version, flags, method (stored), time, date (1980-01-01)
        }
        le32(&mut self.out, crc);
        le32(&mut self.out, len);
        le32(&mut self.out, len);
        le16(&mut self.out, name.len() as u16);
        le16(&mut self.out, 0);
        self.out.extend_from_slice(name.as_bytes());
        self.out.extend_from_slice(data);
        // its central directory entry
        let c = &mut self.central;
        le32(c, 0x0201_4b50);
        for x in [20u16, 20, 0, 0, 0, 0x21] {
            le16(c, x); // made by, needed, flags, method, time, date
        }
        le32(c, crc);
        le32(c, len);
        le32(c, len);
        le16(c, name.len() as u16);
        for x in [0u16, 0, 0, 0] {
            le16(c, x); // extra, comment, disk, internal attributes
        }
        le32(c, 0); // external attributes
        le32(c, off);
        c.extend_from_slice(name.as_bytes());
        self.count += 1;
    }

    fn finish(mut self) -> Vec<u8> {
        let at = self.out.len() as u32;
        let size = self.central.len() as u32;
        self.out.extend_from_slice(&self.central);
        self.out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        self.out.extend_from_slice(&[0, 0, 0, 0]); // disk numbers
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&size.to_le_bytes());
        self.out.extend_from_slice(&at.to_le_bytes());
        self.out.extend_from_slice(&[0, 0]); // comment
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Calibration;

    /// An Extract of buffer words (buffer::unpack) whose current code is `codes` (range 0).
    fn extract(codes: &[u32]) -> Extract {
        let words = codes.iter().map(|&c| (1u32 << 31) | (3300 << 14) | c).collect();
        Extract { runs: vec![(0.0, 5e-7, words, None)], cal: Calibration::default() }
    }

    fn campaign(traces: &[Vec<u32>], groups: &[u8]) -> Campaign {
        let mut c = Campaign::new(Config { samples: traces[0].len(), source: Source::I }).unwrap();
        for (t, &g) in traces.iter().zip(groups) {
            c.add(&extract(t), TraceMeta { group: g, ..Default::default() }).unwrap();
        }
        c
    }

    fn pulse(at: usize, len: usize) -> Vec<u32> {
        (0..len).map(|i| if (at..at + 5).contains(&i) { 4000 } else { 100 + (i as u32 * 7) % 13 }).collect()
    }

    #[test]
    fn short_extract_is_refused() {
        let mut c = Campaign::new(Config { samples: 100, source: Source::I }).unwrap();
        assert!(c.add(&extract(&[1; 50]), TraceMeta::default()).is_err());
        assert!(c.add(&extract(&[1; 120]), TraceMeta::default()).is_ok());
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn align_finds_the_shift() {
        let c0 = pulse(40, 100);
        let c1 = pulse(43, 100);
        let c2 = pulse(37, 100);
        let mut c = campaign(&[c0, c1, c2], &[0, 0, 0]);
        let r = c.align(30, 55, 8).unwrap();
        assert_eq!(r.shifts, vec![0, 3, -3]);
        let Result::Overview { std, .. } = c.query(&Request::Overview { show: 0 }) else { panic!() };
        // aligned, the pulse lines up: no spread at its samples
        assert!(std[41] < 1e-9, "{}", std[41]);
    }

    #[test]
    fn ttest_sees_a_difference() {
        let lo: Vec<u32> = (0..20).map(|i| 100 + (i % 3)).collect();
        let mut hi = lo.clone();
        hi[10] += 50;
        let mut traces = Vec::new();
        let mut groups = Vec::new();
        for k in 0..20u32 {
            let mut a = lo.clone();
            let mut b = hi.clone();
            a[10] += k % 4;
            b[10] += (k + 1) % 4;
            traces.push(a);
            traces.push(b);
            groups.extend([0, 1]);
        }
        let c = campaign(&traces, &groups);
        let Result::TTest { max_abs, at, n0, n1, .. } = c.query(&Request::TTest) else { panic!() };
        assert_eq!((n0, n1, at), (20, 20, 10));
        assert!(max_abs > 4.5);
    }

    #[test]
    fn npz_layout() {
        let c = campaign(&[pulse(5, 16), pulse(6, 16)], &[0, 1]);
        let z = c.export_npz();
        assert_eq!(&z[..4], b"PK\x03\x04");
        let eocd = z.len() - 22;
        assert_eq!(&z[eocd..eocd + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([z[eocd + 10], z[eocd + 11]]), 6);
        // the first entry is traces.npy: magic, header a multiple of 64
        let name_len = u16::from_le_bytes([z[26], z[27]]) as usize;
        let npy = &z[30 + name_len..];
        assert_eq!(&npy[..6], b"\x93NUMPY");
        let hl = u16::from_le_bytes([npy[8], npy[9]]) as usize;
        assert_eq!((10 + hl) % 64, 0);
        assert!(std::str::from_utf8(&npy[10..10 + hl]).unwrap().contains("'shape': (2, 16)"));
    }
}

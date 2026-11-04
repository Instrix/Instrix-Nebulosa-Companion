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
    /// correlation power analysis on one AES key byte: for each of the 256 key
    /// guesses, correlate the Hamming weight of the first-round S-box output
    /// (from `input[byte]`) against every sample, over all traces; the best is
    /// the key byte. A known first-round `key` byte (else 256) is flagged.
    Cpa { byte: usize, known: u16 },
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
    Cpa {
        byte: usize,
        dt: f64,
        /// max |correlation| over samples, one per key guess (256)
        corr: Vec<f32>,
        /// the best guess (argmax), its correlation, and where it peaks
        best: u8,
        best_corr: f32,
        at: usize,
        /// the next-best correlation (the margin tells how sure it is)
        second: f32,
        /// whether `best` matches the known key byte (null when unknown)
        correct: Option<bool>,
    },
    Empty {
        reason: String,
    },
}

/// AES S-box (same table as aes.ts) for the CPA leakage model.
#[rustfmt::skip]
const SBOX: [u8; 256] = [
    0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,
    0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
    0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,
    0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
    0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,
    0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
    0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,
    0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
    0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,
    0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
    0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
];

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
            Request::Cpa { byte, known } => return self.cpa(byte, known),
        }
    }

    /// Correlation power analysis on key byte `byte`: the Hamming-weight model
    /// of the first-round S-box output, correlated against every sample for
    /// each of the 256 guesses; the guess with the highest |correlation| is
    /// the byte. `known` (0..255) flags whether it came out right, else ≥256.
    fn cpa(&self, byte: usize, known: u16) -> Result {
        let n = self.len();
        let l = self.cfg.samples;
        if n < 16 {
            return Result::Empty { reason: "correlation needs more traces (dozens per key byte)".into() };
        }
        if self.meta.iter().any(|m| m.input.len() <= byte) {
            return Result::Empty { reason: format!("the traces have no input byte {byte} (capture AES — random plaintext)") };
        }
        // aligned traces, row by row
        let mut tr = vec![0f32; n * l];
        let mut buf = Vec::with_capacity(l);
        for i in 0..n {
            self.aligned(i, &mut buf);
            tr[i * l..(i + 1) * l].copy_from_slice(&buf);
        }
        // the model: m[i*256 + g] = HW(S(input[byte] ^ g)); its sums per guess
        let mut m = vec![0f32; n * 256];
        let mut sum_m = [0f64; 256];
        let mut sum_m2 = [0f64; 256];
        for i in 0..n {
            let p = self.meta[i].input[byte];
            for g in 0..256 {
                let v = SBOX[(p ^ g as u8) as usize].count_ones() as f32;
                m[i * 256 + g] = v;
                sum_m[g] += v as f64;
                sum_m2[g] += (v * v) as f64;
            }
        }
        let nf = n as f64;
        let mut maxc = [0f32; 256];
        let mut at = [0usize; 256];
        let mut acc = [0f64; 256];
        for s in 0..l {
            acc.iter_mut().for_each(|a| *a = 0.0);
            let mut sum_t = 0f64;
            let mut sum_t2 = 0f64;
            for i in 0..n {
                let t = tr[i * l + s] as f64;
                sum_t += t;
                sum_t2 += t * t;
                let row = &m[i * 256..i * 256 + 256];
                for g in 0..256 {
                    acc[g] += row[g] as f64 * t;
                }
            }
            let dt = nf * sum_t2 - sum_t * sum_t;
            if dt <= 0.0 {
                continue;
            }
            let dts = dt.sqrt();
            for g in 0..256 {
                let dm = nf * sum_m2[g] - sum_m[g] * sum_m[g];
                if dm <= 0.0 {
                    continue;
                }
                let r = ((nf * acc[g] - sum_m[g] * sum_t) / (dm.sqrt() * dts)).abs() as f32;
                if r > maxc[g] {
                    maxc[g] = r;
                    at[g] = s;
                }
            }
        }
        let best = (0..256).max_by(|&a, &b| maxc[a].total_cmp(&maxc[b])).unwrap();
        let best_corr = maxc[best];
        let second = (0..256).filter(|&g| g != best).map(|g| maxc[g]).fold(0f32, f32::max);
        Result::Cpa {
            byte,
            dt: self.dt,
            corr: maxc.to_vec(),
            best: best as u8,
            best_corr,
            at: at[best],
            second,
            correct: (known < 256).then_some(best as u16 == known),
        }
    }

    /// The campaign as a flat binary (.nbsca) that `from_raw` reads back: a
    /// magic, the config, dt, the shifts, the raw traces and the per-trace
    /// metadata. Little-endian throughout.
    pub fn export_raw(&self) -> Vec<u8> {
        let n = self.len();
        let l = self.cfg.samples;
        let mut o = Vec::with_capacity(32 + n * l * 4);
        o.extend_from_slice(b"NBSCA1\0\0");
        o.push(match self.cfg.source {
            Source::I => 0,
            Source::V => 1,
            Source::P => 2,
            Source::M => 3,
        });
        o.extend_from_slice(&(l as u32).to_le_bytes());
        o.extend_from_slice(&(n as u32).to_le_bytes());
        o.extend_from_slice(&self.dt.to_le_bytes());
        for &sh in &self.shifts {
            o.extend_from_slice(&sh.to_le_bytes());
        }
        for &x in &self.data {
            o.extend_from_slice(&x.to_le_bytes());
        }
        for m in &self.meta {
            o.push(m.group);
            o.extend_from_slice(&m.width.to_le_bytes());
            o.extend_from_slice(&(m.input.len() as u16).to_le_bytes());
            o.extend_from_slice(&m.input);
            o.extend_from_slice(&(m.output.len() as u16).to_le_bytes());
            o.extend_from_slice(&m.output);
        }
        o
    }

    /// Rebuild a campaign from `export_raw` bytes.
    pub fn from_raw(b: &[u8]) -> std::result::Result<Self, String> {
        let bad = || "not a Nebulosa side-channel file".to_string();
        if b.len() < 24 || &b[..8] != b"NBSCA1\0\0" {
            return Err(bad());
        }
        let source = match b[8] {
            0 => Source::I,
            1 => Source::V,
            2 => Source::P,
            _ => Source::M,
        };
        let u32at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize;
        let l = u32at(9);
        let n = u32at(13);
        let dt = f64::from_le_bytes(b[17..25].try_into().map_err(|_| bad())?);
        let mut p = 25;
        let need = |p: usize, k: usize| if p + k <= b.len() { Ok(()) } else { Err(bad()) };
        need(p, n * 4 + n * l * 4)?;
        let mut shifts = Vec::with_capacity(n);
        for _ in 0..n {
            shifts.push(i32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]));
            p += 4;
        }
        let mut data = Vec::with_capacity(n * l);
        for _ in 0..n * l {
            data.push(f32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]));
            p += 4;
        }
        let mut meta = Vec::with_capacity(n);
        for _ in 0..n {
            need(p, 11)?;
            let group = b[p];
            let width = f64::from_le_bytes(b[p + 1..p + 9].try_into().map_err(|_| bad())?);
            p += 9;
            let il = u16::from_le_bytes([b[p], b[p + 1]]) as usize;
            p += 2;
            need(p, il + 2)?;
            let input = b[p..p + il].to_vec();
            p += il;
            let ol = u16::from_le_bytes([b[p], b[p + 1]]) as usize;
            p += 2;
            need(p, ol)?;
            let output = b[p..p + ol].to_vec();
            p += ol;
            meta.push(TraceMeta { input, output, group, width });
        }
        Ok(Self { cfg: Config { samples: l, source }, dt, data, shifts, meta })
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

    /// A synthetic AES trace: a flat trace whose sample 10 leaks the Hamming
    /// weight of S(pt ⊕ key) for the given key byte, plus a little noise.
    fn aes_trace(pt: u8, key: u8, seed: u32) -> (Vec<u32>, u8) {
        let leak = SBOX[(pt ^ key) as usize].count_ones() as i64;
        let codes: Vec<u32> = (0..40)
            .map(|i| {
                let base = 200 + if i == 10 { leak * 60 } else { 0 };
                let noise = ((seed.wrapping_mul(2654435761).wrapping_add(i as u32 * 40503) >> 8) % 31) as i64 - 15;
                (base + noise).max(0) as u32
            })
            .collect();
        (codes, pt)
    }

    #[test]
    fn cpa_recovers_a_key_byte() {
        let key = 0x2b;
        let mut c = Campaign::new(Config { samples: 40, source: Source::I }).unwrap();
        for s in 0..200u32 {
            let pt = (s.wrapping_mul(97) & 0xff) as u8;
            let (codes, _) = aes_trace(pt, key, s + 1);
            c.add(&extract(&codes), TraceMeta { input: vec![pt], ..Default::default() }).unwrap();
        }
        let Result::Cpa { best, correct, best_corr, second, .. } = c.query(&Request::Cpa { byte: 0, known: key as u16 }) else {
            panic!()
        };
        assert_eq!(best, key, "corr {best_corr}");
        assert_eq!(correct, Some(true));
        assert!(best_corr > second + 0.1, "best {best_corr} vs second {second}");
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
    fn raw_roundtrip() {
        let c = campaign(&[pulse(5, 16), pulse(9, 16)], &[0, 1]);
        let bytes = c.export_raw();
        let c2 = Campaign::from_raw(&bytes).unwrap();
        assert_eq!(c2.len(), 2);
        assert_eq!(c2.cfg.samples, 16);
        assert_eq!(c2.meta[1].group, 1);
        // the data survives
        let o1 = c.query(&Request::Overview { show: 0 });
        let o2 = c2.query(&Request::Overview { show: 0 });
        let (Result::Overview { mean: m1, .. }, Result::Overview { mean: m2, .. }) = (o1, o2) else { panic!() };
        assert_eq!(m1, m2);
        assert!(Campaign::from_raw(b"nope").is_err());
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

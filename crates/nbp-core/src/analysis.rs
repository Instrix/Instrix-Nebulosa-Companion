//! Analysis over a span of samples: histogram (with percentiles, for the CDF
//! / CCDF), power spectral density (Welch) and the worst window.
//!
//! Work on an [`Extract`]: the span's raw samples copied out of the buffer,
//! so a long analysis doesn't hold the buffer (and the stream) up.

use crate::buffer::unpack;
use crate::trigger::Source;
use crate::units::Calibration;
use serde::{Deserialize, Serialize};

/// Contiguous runs of raw samples: (time of the first, s per sample, packed
/// samples, the math channel's values when there is one).
pub struct Extract {
    pub runs: Vec<(f64, f64, Vec<u32>, Option<Vec<f32>>)>,
    pub cal: Calibration,
}

impl Extract {
    pub fn len(&self) -> usize {
        self.runs.iter().map(|r| r.2.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn value(&self, w: u32, src: Source) -> f64 {
        let (ic, vc, r, _) = unpack(w);
        let i = self.cal.current(ic, r);
        match src {
            Source::I => i,
            Source::V => self.cal.voltage(vc),
            Source::P | Source::M => i * self.cal.voltage(vc),
        }
    }

    /// Every run as (t0, dt, values); the math channel's non-finite values are left out.
    fn values(&self, src: Source) -> impl Iterator<Item = (f64, f64, Box<dyn Iterator<Item = f64> + '_>)> + '_ {
        self.runs.iter().map(move |(t0, dt, s, m)| {
            let it: Box<dyn Iterator<Item = f64> + '_> = match (src, m) {
                (Source::M, Some(m)) => Box::new(m.iter().map(|&x| x as f64).filter(|x| x.is_finite())),
                (Source::M, None) => Box::new(std::iter::empty()),
                _ => Box::new(s.iter().map(move |&w| self.value(w, src))),
            };
            (*t0, *dt, it)
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    /// `bins` bins between the span's min and max; `log`: logarithmic bins
    /// over the positive values (current spans decades)
    Histogram { source: Source, bins: usize, log: bool },
    /// Welch: Hann window, 50 % overlap, `nfft` a power of two
    Psd { source: Source, nfft: usize },
    /// the `width`-long window (s) with the highest mean
    MaxWindow { source: Source, width: f64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Result {
    Histogram {
        /// bin edges (bins + 1)
        edges: Vec<f64>,
        counts: Vec<u64>,
        /// samples counted, and those left out (≤ 0 on a log histogram)
        n: u64,
        excluded: u64,
        mean: f64,
        std: f64,
        min: f64,
        max: f64,
        /// p1, p5, p50, p95, p99 (from the bins)
        percentiles: Vec<(f64, f64)>,
    },
    Psd {
        /// Hz
        freq: Vec<f64>,
        /// unit² / Hz
        psd: Vec<f64>,
        segments: usize,
        /// Hz between bins
        resolution: f64,
        /// rms over the band (√∫psd)
        rms: f64,
    },
    MaxWindow {
        /// start of the window, s
        t: f64,
        width: f64,
        mean: f64,
        /// the span's overall mean, to compare
        span_mean: f64,
    },
    Empty {
        reason: String,
    },
}

pub fn run(x: &Extract, req: &Request) -> Result {
    if x.is_empty() {
        return Result::Empty { reason: "no samples in the span".into() };
    }
    match *req {
        Request::Histogram { source, bins, log } => histogram(x, source, bins.clamp(2, 10_000), log),
        Request::Psd { source, nfft } => psd(x, source, nfft.next_power_of_two().clamp(64, 1 << 20)),
        Request::MaxWindow { source, width } => max_window(x, source, width),
    }
}

fn histogram(x: &Extract, src: Source, bins: usize, log: bool) -> Result {
    // first pass: range and moments
    let (mut n, mut mean, mut m2) = (0u64, 0f64, 0f64);
    let (mut lo, mut hi, mut lo_pos) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY);
    for (_, _, vals) in x.values(src) {
        for v in vals {
            n += 1;
            let d = v - mean;
            mean += d / n as f64;
            m2 += d * (v - mean);
            lo = lo.min(v);
            hi = hi.max(v);
            if v > 0.0 {
                lo_pos = lo_pos.min(v);
            }
        }
    }
    let std = if n > 1 { (m2 / (n - 1) as f64).sqrt() } else { 0.0 };
    let (a, b) = if log {
        if !(hi > 0.0) {
            return Result::Empty { reason: "no positive values for a log histogram".into() };
        }
        (lo_pos.max(hi * 1e-9).ln(), hi.ln())
    } else {
        (lo, hi)
    };
    let b = if b > a { b } else { a + 1e-12_f64.max(a.abs() * 1e-9) };
    let w = (b - a) / bins as f64;
    let mut counts = vec![0u64; bins];
    let mut excluded = 0u64;
    for (_, _, vals) in x.values(src) {
        for v in vals {
            let u = if log {
                if v <= 0.0 {
                    excluded += 1;
                    continue;
                }
                v.ln()
            } else {
                v
            };
            let k = (((u - a) / w) as isize).clamp(0, bins as isize - 1) as usize;
            counts[k] += 1;
        }
    }
    let edges: Vec<f64> = (0..=bins).map(|k| a + k as f64 * w).map(|e| if log { e.exp() } else { e }).collect();
    // percentiles: interpolated inside the bin
    let total: u64 = counts.iter().sum();
    let mut percentiles = Vec::new();
    for p in [0.01, 0.05, 0.5, 0.95, 0.99] {
        let target = p * total as f64;
        let mut acc = 0.0;
        let mut val = edges[bins];
        for k in 0..bins {
            let c = counts[k] as f64;
            if acc + c >= target && c > 0.0 {
                let f = (target - acc) / c;
                val = edges[k] + f * (edges[k + 1] - edges[k]);
                break;
            }
            acc += c;
        }
        percentiles.push((p, val));
    }
    Result::Histogram { edges, counts, n, excluded, mean, std, min: lo, max: hi, percentiles }
}

/// In-place radix-2 FFT (re, im), n a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

const MAX_SEGMENTS: usize = 400;

fn psd(x: &Extract, src: Source, nfft: usize) -> Result {
    let window: Vec<f64> = (0..nfft).map(|k| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / nfft as f64).cos()).collect();
    let wpow: f64 = window.iter().map(|w| w * w).sum();
    let hop = nfft / 2;
    // how many segments fit; above MAX_SEGMENTS, skip some evenly
    let fits: usize = x.runs.iter().map(|r| if r.2.len() >= nfft { (r.2.len() - nfft) / hop + 1 } else { 0 }).sum::<usize>().max(1);
    if fits == 0 {
        return Result::Empty { reason: format!("needs at least {nfft} contiguous samples") };
    }
    let every = fits.div_ceil(MAX_SEGMENTS);
    let mut acc = vec![0.0f64; nfft / 2 + 1];
    let (mut used, mut seen) = (0usize, 0usize);
    let mut dt = 0.0;
    let (mut re, mut im) = (vec![0.0; nfft], vec![0.0; nfft]);
    for (_, rdt, vals) in x.values(src) {
        let v: Vec<f64> = vals.collect();
        if v.len() < nfft {
            continue;
        }
        if dt == 0.0 {
            dt = rdt;
        }
        let mut s = 0;
        while s + nfft <= v.len() {
            if seen % every == 0 && rdt == dt {
                // remove the segment's mean (DC would swamp the low bins' leakage)
                let m = v[s..s + nfft].iter().sum::<f64>() / nfft as f64;
                for k in 0..nfft {
                    re[k] = (v[s + k] - m) * window[k];
                    im[k] = 0.0;
                }
                fft(&mut re, &mut im);
                for k in 0..=nfft / 2 {
                    acc[k] += re[k] * re[k] + im[k] * im[k];
                }
                used += 1;
            }
            seen += 1;
            s += hop;
        }
    }
    let fs = 1.0 / dt;
    // one-sided PSD, unit²/Hz
    let scale = 1.0 / (fs * wpow * used as f64);
    let psd: Vec<f64> = acc
        .iter()
        .enumerate()
        .map(|(k, a)| a * scale * if k == 0 || k == nfft / 2 { 1.0 } else { 2.0 })
        .collect();
    let df = fs / nfft as f64;
    let rms = (psd.iter().skip(1).sum::<f64>() * df).sqrt();
    Result::Psd { freq: (0..=nfft / 2).map(|k| k as f64 * df).collect(), psd, segments: used, resolution: df, rms }
}

fn max_window(x: &Extract, src: Source, width: f64) -> Result {
    let (mut best, mut best_t, mut best_w) = (f64::NEG_INFINITY, f64::NAN, width);
    let (mut total, mut n) = (0.0, 0u64);
    for (t0, dt, vals) in x.values(src) {
        let v: Vec<f64> = vals.collect();
        total += v.iter().sum::<f64>();
        n += v.len() as u64;
        let k = ((width / dt).round() as usize).max(1);
        if v.len() < k {
            continue;
        }
        let mut sum: f64 = v[..k].iter().sum();
        let mut i = 0;
        loop {
            let m = sum / k as f64;
            if m > best {
                best = m;
                best_t = t0 + i as f64 * dt;
                best_w = k as f64 * dt;
            }
            if i + k >= v.len() {
                break;
            }
            sum += v[i + k] - v[i];
            i += 1;
        }
    }
    if best_t.is_nan() {
        return Result::Empty { reason: "the span is shorter than the window".into() };
    }
    Result::MaxWindow { t: best_t, width: best_w, mean: best, span_mean: total / n as f64 }
}

/// For tests: an extract from plain values (current codes, range 0).
#[cfg(test)]
fn extract_of(codes: &[u16], dt: f64) -> Extract {
    let runs = vec![(0.0, dt, codes.iter().map(|&c| crate::buffer::pack(c, 0, 0, false)).collect(), None)];
    Extract { runs, cal: Calibration::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_uniform() {
        let codes: Vec<u16> = (0..16384u32).map(|k| k as u16).collect();
        let x = extract_of(&codes, 1e-6);
        let Result::Histogram { counts, n, percentiles, .. } = run(&x, &Request::Histogram { source: Source::I, bins: 16, log: false })
        else {
            panic!()
        };
        assert_eq!(n, 16384);
        assert!(counts.iter().all(|&c| (c as i64 - 1024).abs() <= 1));
        let p50 = percentiles.iter().find(|p| p.0 == 0.5).unwrap().1;
        assert!((p50 - 1.0).abs() < 0.01, "p50 {p50}"); // 0..2 A
    }

    #[test]
    fn psd_finds_a_tone_and_its_power() {
        // a 1 kHz square-ish tone at 1 MS/s: codes alternate 500 samples
        let codes: Vec<u16> = (0..1 << 18).map(|k| if (k / 500) % 2 == 0 { 8192 + 2000 } else { 8192 - 2000 }).collect();
        let x = extract_of(&codes, 1e-6);
        let Result::Psd { freq, psd, rms, .. } = run(&x, &Request::Psd { source: Source::I, nfft: 16384 }) else { panic!() };
        let k = (1..psd.len()).max_by(|&a, &b| psd[a].partial_cmp(&psd[b]).unwrap()).unwrap();
        assert!((freq[k] - 1000.0).abs() < 2.0 * (freq[1] - freq[0]), "peak at {}", freq[k]);
        // square wave of amplitude 2000 codes × 2 A / 16384: rms = amplitude
        let amp = 2000.0 * 2.0 / 16384.0;
        assert!((rms - amp).abs() / amp < 0.05, "rms {rms} vs {amp}");
    }

    #[test]
    fn max_window_finds_the_burst() {
        let mut codes = vec![100u16; 10_000];
        for c in &mut codes[6000..6100] {
            *c = 5000;
        }
        let x = extract_of(&codes, 1e-6);
        let Result::MaxWindow { t, mean, .. } = run(&x, &Request::MaxWindow { source: Source::I, width: 100e-6 }) else { panic!() };
        assert!((t - 6000e-6).abs() < 1e-9);
        assert!((mean - 5000.0 * 2.0 / 16384.0).abs() < 1e-12);
    }
}

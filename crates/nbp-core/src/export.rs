//! Logger output: configurable fields in CSV, TXT, JSON Lines (and the
//! columns for HDF5), from interval summaries or from every sample.
//!
//! The stream's thread only hands over compact chunks ([`Chunk`]); an
//! [`Exporter`] (on the writer's thread) turns them into rows. Numbers are
//! written fixed-point with integer formatting, fast enough for every sample
//! at 2 MS/s.

use crate::buffer::{unpack, Moments};
use crate::session::LogEntry;
use crate::stream::TS_PERIOD_S;
use crate::units::Calibration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Data {
    /// one row per session-log interval
    Summary,
    /// one row per sample
    Raw,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Format {
    Csv {
        separator: String,
        /// 1,5 instead of 1.5 (with ';' or tab separators)
        decimal_comma: bool,
        header: bool,
    },
    /// fixed-width columns
    Txt { width: usize, header: bool },
    /// one JSON object per line
    Jsonl,
    /// one dataset per field (written by the app)
    Hdf5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeFmt {
    /// seconds since the export started
    Relative,
    /// seconds since 1970 (UTC)
    Unix,
    /// ISO 8601 UTC (text formats)
    Iso,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum What {
    Time,
    /// current, voltage, power, math
    I,
    V,
    P,
    M,
    /// samples in the interval (summaries)
    Count,
    /// the current range (every sample)
    Range,
    /// the dirty flag (every sample)
    Dirty,
    /// ∫ I dt since the export started, C
    Charge,
    /// ∫ P dt since the export started, J
    Energy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stat {
    Mean,
    Min,
    Max,
    Std,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub what: What,
    /// for I / V / P / M summaries (every sample: the value)
    #[serde(default = "mean")]
    pub stat: Stat,
    /// the column's name
    pub label: String,
    /// value × scale is written (1000: A to mA)
    #[serde(default = "one")]
    pub scale: f64,
    /// digits after the point
    #[serde(default = "six")]
    pub decimals: u8,
}

fn mean() -> Stat {
    Stat::Mean
}
fn one() -> f64 {
    1.0
}
fn six() -> u8 {
    6
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportCfg {
    pub data: Data,
    pub format: Format,
    pub time: TimeFmt,
    pub fields: Vec<Field>,
}

/// What the stream's thread hands over.
#[derive(Debug, Clone)]
pub enum Chunk {
    Entries(Vec<LogEntry>),
    /// evenly spaced samples from ts0 (500 ns periods), packed as the buffer
    /// keeps them, with the math channel's values (NaN without one)
    Raw { ts0: u64, step: u32, packed: Vec<u32>, math: Vec<f32> },
    Calibration(Calibration),
}

/// One row's values, before formatting.
struct Row {
    t: f64,
    ch: [[f64; 4]; 4], // I V P M × mean min max std
    n: f64,
    range: f64,
    dirty: f64,
    /// ∫ I dt and ∫ P dt up to and with this row
    q: f64,
    e: f64,
}

pub struct Exporter {
    cfg: ExportCfg,
    cal: Calibration,
    /// unix time − FPGA time, s
    wall_offset: f64,
    /// FPGA time of the first row (Relative time)
    t_first: Option<f64>,
    charge: f64,
    energy: f64,
    pub rows: u64,
    sep: String,
}

/// Writes `v` with `d` decimals (fixed point, integer formatting).
fn fixed(out: &mut Vec<u8>, v: f64, d: u8, comma: bool) {
    if !v.is_finite() {
        out.extend_from_slice(if v.is_nan() { b"NaN" } else if v > 0.0 { b"inf" } else { b"-inf" });
        return;
    }
    const P: [f64; 13] = [1.0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12];
    let d = d.min(12) as usize;
    let scaled = (v * P[d]).round();
    if scaled.abs() >= 9.0e18 {
        // too big for the integer path
        out.extend_from_slice(format!("{v:.d$}").as_bytes());
        return;
    }
    let mut x = scaled as i64;
    if x < 0 {
        out.push(b'-');
        x = -x;
    }
    let mut buf = itoa::Buffer::new();
    let digits = buf.format(x as u64).as_bytes();
    if d == 0 {
        out.extend_from_slice(digits);
        return;
    }
    let point = if comma { b',' } else { b'.' };
    if digits.len() > d {
        out.extend_from_slice(&digits[..digits.len() - d]);
        out.push(point);
        out.extend_from_slice(&digits[digits.len() - d..]);
    } else {
        out.push(b'0');
        out.push(point);
        out.extend(std::iter::repeat_n(b'0', d - digits.len()));
        out.extend_from_slice(digits);
    }
}

/// ISO 8601 UTC with microseconds.
fn iso(out: &mut Vec<u8>, unix: f64) {
    let secs = unix.floor() as i64;
    let us = ((unix - secs as f64) * 1e6).round().min(999_999.0) as u32;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    out.extend_from_slice(
        format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{us:06}Z", rem / 3600, rem % 3600 / 60, rem % 60).as_bytes(),
    );
}

impl Exporter {
    pub fn new(cfg: ExportCfg, cal: Calibration, wall_offset: f64) -> Self {
        let sep = match &cfg.format {
            Format::Csv { separator, .. } => {
                if separator.is_empty() {
                    ",".into()
                } else {
                    separator.replace("\\t", "\t")
                }
            }
            _ => String::new(),
        };
        Self { cfg, cal, wall_offset, t_first: None, charge: 0.0, energy: 0.0, rows: 0, sep }
    }

    pub fn cfg(&self) -> &ExportCfg {
        &self.cfg
    }

    /// The file's first bytes (a header row, for formats that have one).
    pub fn header(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match &self.cfg.format {
            Format::Csv { header: true, .. } => {
                let labels: Vec<String> = self.cfg.fields.iter().map(|f| csv_quote(&f.label, &self.sep)).collect();
                out.extend_from_slice(labels.join(&self.sep).as_bytes());
                out.push(b'\n');
            }
            Format::Txt { width, header: true } => {
                for (k, f) in self.cfg.fields.iter().enumerate() {
                    let w = self.width(k, *width);
                    out.extend_from_slice(format!("{:>w$}", f.label, w = w).as_bytes());
                    if k + 1 < self.cfg.fields.len() {
                        out.push(b' ');
                    }
                }
                out.push(b'\n');
            }
            _ => {}
        }
        out
    }

    fn width(&self, k: usize, width: usize) -> usize {
        if self.cfg.fields[k].what == What::Time && self.cfg.time == TimeFmt::Iso {
            width.max(27)
        } else {
            width
        }
    }

    /// Rows for a chunk, formatted (text formats).
    pub fn text(&mut self, c: &Chunk, out: &mut Vec<u8>) {
        let mut rows = Vec::new();
        self.rows_of(c, &mut |r| rows.push(r));
        for r in rows {
            self.write_row(&r, out);
        }
    }

    /// Columns for a chunk, one per field (HDF5); time as configured but never ISO (seconds).
    pub fn columns(&mut self, c: &Chunk) -> Vec<Vec<f64>> {
        let mut cols: Vec<Vec<f64>> = vec![Vec::new(); self.cfg.fields.len()];
        let mut rows = Vec::new();
        self.rows_of(c, &mut |r| rows.push(r));
        let fields = std::mem::take(&mut self.cfg.fields);
        for r in &rows {
            for (k, f) in fields.iter().enumerate() {
                cols[k].push(self.value(f, r));
            }
        }
        self.cfg.fields = fields;
        cols
    }

    fn rows_of(&mut self, c: &Chunk, f: &mut dyn FnMut(Row)) {
        match c {
            Chunk::Calibration(cal) => self.cal = cal.clone(),
            Chunk::Entries(es) => {
                for e in es {
                    let m = e.moments3();
                    let col = |q: &Moments| {
                        if q.n == 0 {
                            [f64::NAN; 4]
                        } else {
                            [q.mean, q.min, q.max, if q.n > 1 { (q.m2 / (q.n - 1) as f64).sqrt() } else { 0.0 }]
                        }
                    };
                    self.charge += m.i.mean * e.dur;
                    self.energy += m.p.mean * e.dur;
                    self.rows += 1;
                    f(Row {
                        t: e.t0,
                        ch: [col(&m.i), col(&m.v), col(&m.p), col(&m.m)],
                        n: m.n() as f64,
                        range: f64::NAN,
                        dirty: f64::NAN,
                        q: self.charge,
                        e: self.energy,
                    });
                }
            }
            Chunk::Raw { ts0, step, packed, math } => {
                let dt = *step as f64 * TS_PERIOD_S;
                for (k, &w) in packed.iter().enumerate() {
                    let (ic, vc, r, d) = unpack(w);
                    let i = self.cal.current(ic, r);
                    let v = self.cal.voltage(vc);
                    let p = i * v;
                    let m = math.get(k).map_or(f64::NAN, |&x| x as f64);
                    self.charge += i * dt;
                    self.energy += p * dt;
                    self.rows += 1;
                    let t = (ts0 + k as u64 * *step as u64) as f64 * TS_PERIOD_S;
                    f(Row { t, ch: [[i; 4], [v; 4], [p; 4], [m; 4]], n: 1.0, range: r as f64, dirty: d as u8 as f64, q: self.charge, e: self.energy });
                }
            }
        }
    }

    /// A field's number for a row (time in seconds, as configured; ISO as unix seconds).
    fn value(&mut self, f: &Field, r: &Row) -> f64 {
        let s = |k: usize| {
            let st = match f.stat {
                Stat::Mean => 0,
                Stat::Min => 1,
                Stat::Max => 2,
                Stat::Std => 3,
            };
            r.ch[k][st] * f.scale
        };
        match f.what {
            What::Time => {
                let t0 = *self.t_first.get_or_insert(r.t);
                match self.cfg.time {
                    TimeFmt::Relative => (r.t - t0) * f.scale,
                    _ => r.t + self.wall_offset,
                }
            }
            What::I => s(0),
            What::V => s(1),
            What::P => s(2),
            What::M => s(3),
            What::Count => r.n,
            What::Range => r.range,
            What::Dirty => r.dirty,
            What::Charge => r.q * f.scale,
            What::Energy => r.e * f.scale,
        }
    }

    fn write_row(&mut self, r: &Row, out: &mut Vec<u8>) {
        // (fields and the separator are taken out and put back: no allocation per row)
        let fields = std::mem::take(&mut self.cfg.fields);
        let comma = matches!(self.cfg.format, Format::Csv { decimal_comma: true, .. });
        let txt_width = match self.cfg.format {
            Format::Txt { width, .. } => Some(width),
            _ => None,
        };
        let jsonl = matches!(self.cfg.format, Format::Jsonl);
        match () {
            _ if jsonl => {
                out.push(b'{');
                for (k, f) in fields.iter().enumerate() {
                    if k > 0 {
                        out.push(b',');
                    }
                    out.extend_from_slice(serde_json::to_string(&f.label).unwrap_or_default().as_bytes());
                    out.push(b':');
                    self.cell(f, r, out, false, true);
                }
                out.extend_from_slice(b"}\n");
            }
            _ if txt_width.is_some() => {
                let width = txt_width.unwrap();
                let mut cell = Vec::with_capacity(32);
                for (k, f) in fields.iter().enumerate() {
                    cell.clear();
                    self.cell(f, r, &mut cell, false, false);
                    let w = if f.what == What::Time && self.cfg.time == TimeFmt::Iso { width.max(27) } else { width };
                    out.extend(std::iter::repeat_n(b' ', w.saturating_sub(cell.len())));
                    out.extend_from_slice(&cell);
                    if k + 1 < fields.len() {
                        out.push(b' ');
                    }
                }
                out.push(b'\n');
            }
            _ => {
                let sep = std::mem::take(&mut self.sep);
                for (k, f) in fields.iter().enumerate() {
                    if k > 0 {
                        out.extend_from_slice(sep.as_bytes());
                    }
                    self.cell(f, r, out, comma, false);
                }
                out.push(b'\n');
                self.sep = sep;
            }
        }
        self.cfg.fields = fields;
    }

    fn cell(&mut self, f: &Field, r: &Row, out: &mut Vec<u8>, comma: bool, json: bool) {
        let v = self.value(f, r);
        if f.what == What::Time && self.cfg.time == TimeFmt::Iso {
            if json {
                out.push(b'"');
            }
            iso(out, v);
            if json {
                out.push(b'"');
            }
        } else if json && !v.is_finite() {
            out.extend_from_slice(b"null"); // JSON has no NaN
        } else {
            let d = if matches!(f.what, What::Count | What::Range | What::Dirty) { 0 } else { f.decimals };
            fixed(out, v, d, comma);
        }
    }
}

fn csv_quote(s: &str, sep: &str) -> String {
    if s.contains(sep) || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Collects samples into [`Chunk::Raw`] runs (a new one at a gap or a
/// decimation change, or every 64 Ki samples).
#[derive(Default)]
pub struct RawTap {
    ts0: u64,
    step: u32,
    packed: Vec<u32>,
    math: Vec<f32>,
    pub done: Vec<Chunk>,
}

const TAP_BLOCK: usize = 1 << 16;

impl RawTap {
    #[inline]
    pub fn sample(&mut self, ts: u64, step: u32, packed: u32, m: f64) {
        let n = self.packed.len() as u64;
        if n > 0 && (step != self.step || self.ts0 + n * self.step as u64 != ts) {
            self.flush();
        }
        if self.packed.is_empty() {
            self.ts0 = ts;
            self.step = step;
        }
        self.packed.push(packed);
        self.math.push(m as f32);
        if self.packed.len() == TAP_BLOCK {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if self.packed.is_empty() {
            return;
        }
        let packed = std::mem::replace(&mut self.packed, Vec::with_capacity(TAP_BLOCK));
        let math = std::mem::replace(&mut self.math, Vec::with_capacity(TAP_BLOCK));
        self.done.push(Chunk::Raw { ts0: self.ts0, step: self.step, packed, math });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::pack;

    fn cfg(format: Format, time: TimeFmt) -> ExportCfg {
        ExportCfg {
            data: Data::Raw,
            format,
            time,
            fields: vec![
                Field { what: What::Time, stat: Stat::Mean, label: "t".into(), scale: 1.0, decimals: 7 },
                Field { what: What::I, stat: Stat::Mean, label: "I (mA)".into(), scale: 1e3, decimals: 4 },
                Field { what: What::V, stat: Stat::Mean, label: "V".into(), scale: 1.0, decimals: 3 },
                Field { what: What::Range, stat: Stat::Mean, label: "range".into(), scale: 1.0, decimals: 0 },
            ],
        }
    }

    fn raw() -> Chunk {
        // 8192 codes in range 0 (2 A full scale): 1 A; 3300 mV
        Chunk::Raw { ts0: 0, step: 1, packed: vec![pack(8192, 3300, 0, false), pack(4096, 3301, 0, true)], math: vec![f32::NAN; 2] }
    }

    #[test]
    fn fixed_point() {
        let mut o = Vec::new();
        fixed(&mut o, -0.0012345, 6, false);
        o.push(b' ');
        fixed(&mut o, 12.5, 2, true);
        o.push(b' ');
        fixed(&mut o, 3.0, 0, false);
        assert_eq!(String::from_utf8(o).unwrap(), "-0.001235 12,50 3");
    }

    #[test]
    fn csv_semicolon_decimal_comma() {
        let mut e = Exporter::new(
            cfg(Format::Csv { separator: ";".into(), decimal_comma: true, header: true }, TimeFmt::Relative),
            Calibration::default(),
            0.0,
        );
        let mut out = e.header();
        e.text(&raw(), &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "t;I (mA);V;range\n0,0000000;1000,0000;3,300;0\n0,0000005;500,0000;3,301;0\n");
    }

    #[test]
    fn jsonl_iso_and_txt() {
        let mut e = Exporter::new(cfg(Format::Jsonl, TimeFmt::Iso), Calibration::default(), 1_700_000_000.0);
        let mut out = Vec::new();
        e.text(&raw(), &mut out);
        let first = String::from_utf8(out).unwrap().lines().next().unwrap().to_string();
        assert_eq!(first, r#"{"t":"2023-11-14T22:13:20.000000Z","I (mA)":1000.0000,"V":3.300,"range":0}"#);
        let mut t = Exporter::new(cfg(Format::Txt { width: 12, header: true }, TimeFmt::Relative), Calibration::default(), 0.0);
        let mut out = t.header();
        t.text(&raw(), &mut out);
        let s = String::from_utf8(out).unwrap();
        assert!(s.lines().all(|l| l.len() == 12 * 4 + 3), "{s}");
    }

    #[test]
    fn summaries_and_integrals() {
        let mut m = crate::buffer::Moments3::default();
        for _ in 0..64 {
            m.add(0.5, 2.0, f64::NAN);
        }
        let e = crate::session::LogEntry::from_m_pub(10.0, 0.1, &m);
        let c = ExportCfg {
            data: Data::Summary,
            format: Format::Csv { separator: ",".into(), decimal_comma: false, header: false },
            time: TimeFmt::Relative,
            fields: vec![
                Field { what: What::I, stat: Stat::Max, label: "Imax".into(), scale: 1.0, decimals: 3 },
                Field { what: What::Count, stat: Stat::Mean, label: "n".into(), scale: 1.0, decimals: 0 },
                Field { what: What::Charge, stat: Stat::Mean, label: "Q".into(), scale: 1.0, decimals: 4 },
            ],
        };
        let mut x = Exporter::new(c, Calibration::default(), 0.0);
        let mut out = Vec::new();
        x.text(&Chunk::Entries(vec![e, e]), &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "0.500,64,0.0500\n0.500,64,0.1000\n");
    }

    #[test]
    fn raw_tap_runs() {
        let mut t = RawTap::default();
        for k in 0..10u64 {
            t.sample(k, 1, 0, f64::NAN);
        }
        t.sample(100, 1, 0, f64::NAN);
        t.flush();
        assert_eq!(t.done.len(), 2);
    }
}

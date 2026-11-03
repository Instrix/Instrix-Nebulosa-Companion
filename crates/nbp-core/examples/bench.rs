//! Pipeline throughput, offline: synthetic NBP DATA (format 5, ranges
//! changing like a real load) pushed as fast as it goes.
//!   cargo run --release -p nbp-core --example bench -- [M samples]
use nbp_core::{msg, Pipeline};
use std::time::Instant;

fn data(n_samples: usize) -> Vec<u8> {
    let mut words: Vec<u32> = Vec::with_capacity(n_samples + n_samples / 100);
    let (mut ts, mut bseq, mut range, mut in_block) = (0u64, 0u32, 9u8, 0);
    let mut x = 12345u32;
    for k in 0..n_samples {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let r = if (k / 20_000) % 25 == 0 { 2 } else { 4 }; // a burst now and then
        if r != range || in_block == 256 {
            range = r;
            in_block = 0;
            words.push((2 << 29) | ((bseq & 0xFFFF) << 11) | ((r as u32) << 2) | 2);
            words.push((2 << 29) | (1 << 27) | ((ts >> 24) as u32 & 0xFF_FFFF));
            words.push((2 << 29) | (2 << 27) | (ts as u32 & 0xFF_FFFF));
            bseq += 1;
        }
        let ic = 8000 + (x & 0x3FF);
        let vc = 3300 + ((x >> 10) & 0x7);
        words.push((1 << 31) | (ic << 14) | vc);
        in_block += 1;
        ts += 1;
    }
    let mut out = Vec::with_capacity(words.len() * 4 + words.len() / 507 * 30);
    for (seq, chunk) in words.chunks(msg::BLOCK_WORDS).enumerate() {
        let mut p = vec![0u8; 2040];
        p[0..4].copy_from_slice(&(seq as u32).to_le_bytes());
        p[8..10].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
        p[10..12].copy_from_slice(&(if seq == 0 { msg::BLK_FIRST } else { 0 }).to_le_bytes());
        for (k, w) in chunk.iter().enumerate() {
            p[12 + k * 4..16 + k * 4].copy_from_slice(&w.to_be_bytes());
        }
        let mut m = nbp_core::encode(msg::DATA, seq as u16, &[]);
        m[6..8].copy_from_slice(&2040u16.to_le_bytes());
        m.extend_from_slice(&p);
        out.extend(m);
    }
    out
}

fn run(label: &str, bytes: &[u8], n: usize, setup: impl Fn(&mut Pipeline)) {
    let mut best = f64::MAX;
    for _ in 0..3 {
        let mut p = Pipeline::new();
        p.set_buffer_samples(1 << 24);
        setup(&mut p);
        let t = Instant::now();
        for c in bytes.chunks(16384) {
            p.push(0, c);
        }
        best = best.min(t.elapsed().as_secs_f64());
        assert!(p.meta().run.n() as usize >= n - 1000);
    }
    println!("{label:<28} {:7.1} MS/s  ({:4.1} % of a core at 2 MS/s)", n as f64 / best / 1e6, 2e6 / (n as f64 / best) * 100.0);
}

fn main() {
    let m: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let n = m * 1_000_000;
    let bytes = data(n);
    run("plain", &bytes, n, |_| {});
    run("math P*1000", &bytes, n, |p| p.set_math(Some("P*1000")).unwrap());
    run("math avg(I, 1ms)", &bytes, n, |p| p.set_math(Some("avg(I, 1ms)")).unwrap());
    run("trigger edge", &bytes, n, |p| {
        p.set_trigger(nbp_core::trigger::TriggerCfg { enabled: true, ..Default::default() })
    });

    // queries, as the scope makes them (~30 / s live)
    let mut p = Pipeline::new();
    p.set_buffer_samples(n);
    for c in bytes.chunks(16384) {
        p.push(0, c);
    }
    let m = p.meta();
    let total = m.t_end - m.t_start;
    for (label, span) in [("lod whole buffer", total), ("lod 1/10", total / 10.0), ("lod 1/1000", total / 1000.0), ("lod 2000 samples", 2000.0 * 5e-7)] {
        let k = 50;
        let t = Instant::now();
        for _ in 0..k {
            std::hint::black_box(p.lod(m.t_end - span, m.t_end, 1500));
        }
        let ms = t.elapsed().as_secs_f64() * 1e3 / k as f64;
        println!("{label:<28} {ms:7.2} ms  ({:4.1} % of a core at 30 / s)", ms * 30.0 / 10.0);
    }
    // formatting every sample (the writer's thread), 4 columns
    use nbp_core::export::*;
    let chunks: Vec<Chunk> = (0..n / 65536)
        .map(|k| Chunk::Raw {
            ts0: (k * 65536) as u64,
            step: 1,
            packed: (0..65536u32).map(|j| nbp_core::buffer::pack((8000 + j % 999) as u16, 3300, 4, false)).collect(),
            math: vec![f32::NAN; 65536],
        })
        .collect();
    let fields = vec![
        Field { what: What::Time, stat: Stat::Mean, label: "t".into(), scale: 1.0, decimals: 7 },
        Field { what: What::I, stat: Stat::Mean, label: "I".into(), scale: 1e6, decimals: 3 },
        Field { what: What::V, stat: Stat::Mean, label: "V".into(), scale: 1.0, decimals: 4 },
        Field { what: What::P, stat: Stat::Mean, label: "P".into(), scale: 1e3, decimals: 4 },
    ];
    for (label, format) in [
        ("csv every sample", Format::Csv { separator: ",".into(), decimal_comma: false, header: true }),
        ("jsonl every sample", Format::Jsonl),
    ] {
        let mut x = Exporter::new(ExportCfg { data: Data::Raw, format, time: TimeFmt::Relative, fields: fields.clone() }, Default::default(), 0.0);
        let mut out = Vec::with_capacity(1 << 26);
        let t = Instant::now();
        let mut bytes = 0usize;
        for c in &chunks {
            out.clear();
            x.text(c, &mut out);
            bytes += out.len();
        }
        let s = t.elapsed().as_secs_f64();
        let rate = (chunks.len() * 65536) as f64 / s;
        println!("{label:<28} {:7.1} M rows/s ({:4.1} % of a core at 2 MS/s), {:.0} B/row = {:.0} MB/s at 2 MS/s",
            rate / 1e6, 2e6 / rate * 100.0, bytes as f64 / (chunks.len() * 65536) as f64, bytes as f64 / (chunks.len() * 65536) as f64 * 2.0);
    }

    let t = Instant::now();
    for _ in 0..50 {
        std::hint::black_box(p.stats(m.t_start, m.t_end));
    }
    println!("{:<28} {:7.3} ms", "stats whole buffer", t.elapsed().as_secs_f64() * 1e3 / 50.0);
}

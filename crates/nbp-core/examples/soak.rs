//! Soak test of the session log: days of logging in seconds, memory and file size.
//!   cargo run --release -p nbp-core --example soak -- [days] [out.nblog]
use nbp_core::buffer::Moments3;
use nbp_core::session::{LogFile, LogWriter, SessionLog};
use std::io::Write;

fn rss_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: f64 = s.split_whitespace().nth(1).and_then(|x| x.parse().ok()).unwrap_or(0.0);
    pages * 4096.0 / 1e6
}

fn main() {
    let days: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let out = std::env::args().nth(2).unwrap_or("/tmp/soak.nblog".into());
    let mut s = SessionLog::new(0.1);
    let mut w = LogWriter::new("{\"soak\":true}");
    let mut f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    f.write_all(&w.take()).unwrap();
    // a 64-sample summary every 32 ms (2 kS/s), current 1 mA ± a burst every 10 s
    let step = 0.032;
    let n = (days * 86400.0 / step) as u64;
    let t = std::time::Instant::now();
    let mut direct = 0.0f64;
    for k in 0..n {
        let t0 = k as f64 * step;
        let i = if (t0 % 10.0) < 0.5 { 0.025 } else { 0.001 };
        let mut m = Moments3::default();
        m.i = nbp_core::buffer::Moments { n: 64, mean: i, m2: 1e-12, min: i * 0.9, max: i * 1.1 };
        m.v = nbp_core::buffer::Moments { n: 64, mean: 3.3, m2: 0.0, min: 3.3, max: 3.3 };
        m.p = nbp_core::buffer::Moments { n: 64, mean: i * 3.3, m2: 0.0, min: i * 3.0, max: i * 3.6 };
        s.add(t0, step, &m);
        direct += i * step;
        let done = s.take_pending();
        if !done.is_empty() {
            w.add(&done);
            f.write_all(&w.take()).unwrap();
        }
        if k % (n / 6).max(1) == 0 && k > 0 {
            let info = s.info();
            println!(
                "day {:.2}: {} entries made, log memory ~{:.0} MB, process RSS {:.0} MB, file {:.0} MB",
                t0 / 86400.0, info.entries, info.bytes as f64 / 1e6, rss_mb(), w.bytes as f64 / 1e6
            );
        }
    }
    s.close();
    w.add(&s.take_pending());
    f.write_all(&w.take()).unwrap();
    f.write_all(&w.finish()).unwrap();
    drop(f);
    let st = s.stats(0.0, days * 86400.0);
    let expect = direct;
    println!("{:.1} s; charge {:.6} C (expected {:.6}); RSS {:.0} MB", t.elapsed().as_secs_f64(), st.charge, expect, rss_mb());
    let t2 = std::time::Instant::now();
    let lf = LogFile::parse(&std::fs::read(&out).unwrap()).unwrap();
    println!("file: {} entries, {:.0} MB, read in {:.0} ms, complete: {}", lf.entries.len(), std::fs::metadata(&out).unwrap().len() as f64 / 1e6, t2.elapsed().as_secs_f64() * 1e3, !lf.truncated);
    let t3 = std::time::Instant::now();
    let l = s.lod(0.0, days * 86400.0, 1500);
    println!("lod over all of it: {} rows in {:.1} ms", l[1], t3.elapsed().as_secs_f64() * 1e3);
}

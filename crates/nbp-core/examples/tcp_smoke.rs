//! Smoke test against a device or nebulosa-sim over TCP:
//!   cargo run --release -p nbp-core --example tcp_smoke -- 127.0.0.1 [seconds]
//! GET_INFO, then a full-rate stream for a few seconds through the Pipeline.

use nbp_core::{encode, msg, Frame, Pipeline};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn request(s: &mut TcpStream, p: &mut Pipeline, seq: u16, cmd: u8, payload: &[u8]) -> Frame {
    s.write_all(&encode(cmd, seq, payload)).unwrap();
    let mut buf = [0u8; 65536];
    let t = Instant::now();
    loop {
        if let Some(f) = p.take_responses().into_iter().find(|f| f.seq == seq) {
            assert_eq!(f.status, 0, "cmd 0x{cmd:02X}: {}", msg::status::name(f.status));
            return f;
        }
        assert!(t.elapsed() < Duration::from_secs(2), "no response to 0x{cmd:02X}");
        if let Ok(n) = s.read(&mut buf) {
            assert!(n > 0, "closed");
            p.push(0, &buf[..n]);
        }
    }
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or("127.0.0.1".into());
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut s = TcpStream::connect((host.as_str(), msg::TCP_PORT)).expect("connect");
    s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let mut p = Pipeline::new();

    let info = msg::Info::parse(&request(&mut s, &mut p, 1, msg::cmd::GET_INFO, &[]).payload).unwrap();
    println!("{} fw {} FPGA {:08X}", info.link_name, info.fw_version_str, info.fpga_id);
    request(&mut s, &mut p, 2, msg::cmd::SET_SOURCE, &msg::enc_source(msg::SRC_STUB_RANDOM));
    request(&mut s, &mut p, 3, msg::cmd::SET_DECIMATION, &msg::enc_decim(0, 0));
    request(&mut s, &mut p, 4, msg::cmd::STREAM_START, &msg::enc_stream_start(msg::START_FLUSH));

    let mut buf = vec![0u8; 1 << 16];
    let t = Instant::now();
    let mut busy = Duration::ZERO;
    while t.elapsed() < Duration::from_secs(secs) {
        if let Ok(n) = s.read(&mut buf) {
            let t0 = Instant::now();
            p.push(0, &buf[..n]);
            busy += t0.elapsed();
        }
    }
    request(&mut s, &mut p, 5, msg::cmd::STREAM_STOP, &[]);
    let m = p.meta();
    let el = t.elapsed().as_secs_f64();
    println!("{:#?}", m.stats);
    println!(
        "{:.2} MB/s, {:.0} samples/s; pipeline busy {:.1} % of the time",
        m.stats.bytes as f64 / el / 1e6,
        m.stats.stream.samples as f64 / el,
        busy.as_secs_f64() / el * 100.0
    );
    println!("buffer {:.3}..{:.3} s, run n {} mean I {:.4e} A", m.t_start, m.t_end, m.run.n(), m.run.i.mean);
    println!("segments {}", p.segments());
    for (span, bins) in [(m.t_end - m.t_start, 1500), (0.1, 1500), (0.001, 1500)] {
        let t0 = Instant::now();
        for _ in 0..10 {
            std::hint::black_box(p.lod(m.t_end - span, m.t_end, bins));
        }
        println!("lod {span:.3} s / {bins}: {:.2} ms", t0.elapsed().as_secs_f64() * 100.0);
    }
    let t0 = Instant::now();
    let l = p.lod(m.t_start, m.t_end, 1500);
    let lod_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t0 = Instant::now();
    let st = p.stats(m.t_start, m.t_end + 1.0);
    let stats_ms = t0.elapsed().as_secs_f64() * 1e3;
    println!("lod 1500 columns over the buffer: {} rows in {lod_ms:.2} ms; stats in {stats_ms:.2} ms (n {}, charge {:.4e} C vs accum {:.4e} C)",
        l[1], st.m.n(), st.charge, m.accum.charge);
}

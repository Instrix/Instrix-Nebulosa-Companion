//! Smoke test against a device or nebulosa-sim over TCP:
//!   cargo run -p nbp-core --example tcp_smoke -- 127.0.0.1 [seconds]
//! GET_INFO, then a stream for a few seconds through the Pipeline.

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
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0, "closed");
        p.push(0, &buf[..n]);
    }
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or("127.0.0.1".into());
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut s = TcpStream::connect((host.as_str(), msg::TCP_PORT)).expect("connect");
    s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let mut p = Pipeline::new();
    p.set_bucket(500);

    let info = msg::Info::parse(&request(&mut s, &mut p, 1, msg::cmd::GET_INFO, &[]).payload).unwrap();
    println!("{info:#?}");
    request(&mut s, &mut p, 2, msg::cmd::SET_SOURCE, &msg::enc_source(msg::SRC_STUB_RANDOM));
    let d = msg::Decim::parse(&request(&mut s, &mut p, 3, msg::cmd::SET_DECIMATION, &msg::enc_decim(0, 0)).payload);
    println!("decimation: {d:?}");
    request(&mut s, &mut p, 4, msg::cmd::STREAM_START, &msg::enc_stream_start(msg::START_FLUSH));

    let mut buf = vec![0u8; 1 << 16];
    let (t, mut points) = (Instant::now(), 0usize);
    while t.elapsed() < Duration::from_secs(secs) {
        if let Ok(n) = s.read(&mut buf) {
            p.push(0, &buf[..n]);
        }
        if let Some(b) = p.take_batch() {
            points += b.len();
        }
    }
    request(&mut s, &mut p, 5, msg::cmd::STREAM_STOP, &[]);
    let st = p.stats();
    println!("{st:#?}");
    let a = p.meta().accum;
    println!("charge {:.6e} C, energy {:.6e} J over {:.3} s", a.charge, a.energy, a.seconds);
    println!(
        "points {points}, {:.2} MB/s, {:.0} samples/s",
        st.bytes as f64 / t.elapsed().as_secs_f64() / 1e6,
        st.stream.samples as f64 / t.elapsed().as_secs_f64()
    );
}

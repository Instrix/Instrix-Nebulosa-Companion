//! Board commands against a device or nebulosa-sim over TCP:
//!   cargo run -p nbp-core --example board_smoke -- 127.0.0.1
//! BOARD_STATUS, the analog supplies on and off, the RTC, the self-tests.

use nbp_core::{board, encode, msg, Frame, Pipeline};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn request(s: &mut TcpStream, p: &mut Pipeline, seq: u16, cmd: u8, payload: &[u8]) -> Frame {
    s.write_all(&encode(cmd, seq, payload)).unwrap();
    let mut buf = [0u8; 65536];
    let t = Instant::now();
    loop {
        if let Some(f) = p.take_responses().into_iter().find(|f| f.seq == seq) {
            return f;
        }
        assert!(t.elapsed() < Duration::from_secs(5), "no response to 0x{cmd:02X}");
        if let Ok(n) = s.read(&mut buf) {
            assert!(n > 0, "closed");
            p.push(0, &buf[..n]);
        }
    }
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or("127.0.0.1".into());
    let mut s = TcpStream::connect((host.as_str(), msg::TCP_PORT)).expect("connect");
    s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let mut p = Pipeline::new();
    let show = |f: &Frame| {
        let b = board::BoardStatus::parse(&f.payload).expect("status payload");
        println!(
            "  [{}] in: jack {} mV, VBUS {} mV, main {} mV; APWR ok {} H {} L {}; PD state {} {} mV / {} mA, pdos {:?}; rtc {} s; up {} ms",
            msg::status::name(f.status), b.v_bj_mv, b.v_vbus_mv, b.v_main_mv, b.apwr_ok, b.apwr_h, b.apwr_l,
            b.pd_state, b.pd_mv, b.pd_ma, b.pd_pdos.iter().map(|d| (d.mv, d.ma)).collect::<Vec<_>>(), b.rtc_s, b.uptime_ms
        );
        b
    };
    println!("status");
    show(&request(&mut s, &mut p, 1, board::CMD_STATUS, &[]));
    println!("APWR_H on");
    show(&request(&mut s, &mut p, 2, board::CMD_SET, &board::enc_set(board::set::APWR_H, 1)));
    println!("APWR_H off");
    show(&request(&mut s, &mut p, 3, board::CMD_SET, &board::enc_set(board::set::APWR_H, 0)));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as u32;
    println!("RTC = now ({now})");
    let b = show(&request(&mut s, &mut p, 4, board::CMD_SET, &board::enc_set(board::set::RTC, now)));
    assert!((b.rtc_s as i64 - now as i64).abs() < 3);
    println!("self-test");
    let b = show(&request(&mut s, &mut p, 5, board::CMD_TEST, &board::enc_test(board::TEST_ALL)));
    for t in &b.tests {
        println!("  {:12} {:5} {} {}", t.name, t.result, t.detail, t.detail_unit);
    }
}

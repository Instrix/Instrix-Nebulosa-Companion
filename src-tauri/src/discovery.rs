//! UDP discovery: GET_INFO (header only) broadcast to port 20035; every
//! device answers with its GET_INFO response in one datagram.

use nbp_core::{msg, FrameParser};
use serde::Serialize;
use std::time::Duration;
use tokio::net::UdpSocket;

#[derive(Serialize)]
pub struct NetDev {
    /// where the answer came from: connect here
    pub host: String,
    pub info: msg::Info,
}

pub async fn discover(timeout: Duration) -> Result<Vec<NetDev>, String> {
    let sock = UdpSocket::bind("0.0.0.0:0").await.map_err(|e| e.to_string())?;
    sock.set_broadcast(true).map_err(|e| e.to_string())?;
    let q = nbp_core::encode(msg::cmd::GET_INFO, 0, &[]);
    sock.send_to(&q, ("255.255.255.255", msg::UDP_DISCOVERY_PORT)).await.map_err(|e| e.to_string())?;

    let mut found: Vec<NetDev> = Vec::new();
    let mut buf = [0u8; 1500];
    let deadline = tokio::time::Instant::now() + timeout;
    while let Ok(Ok((n, from))) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await {
        let mut p = FrameParser::new();
        p.push(&buf[..n]);
        let Some(f) = p.next_frame() else { continue };
        if f.ty != msg::rsp(msg::cmd::GET_INFO) {
            continue;
        }
        let Some(info) = msg::Info::parse(&f.payload) else { continue };
        let host = from.ip().to_string();
        if !found.iter().any(|d| d.host == host) {
            found.push(NetDev { host, info });
        }
    }
    Ok(found)
}

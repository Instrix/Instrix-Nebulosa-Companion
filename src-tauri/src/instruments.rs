//! Bench instruments for the bring-up (the Debug tab): one text line out,
//! optionally one line back, over the instrument's own port. The SCPI and
//! the procedures are the UI's (ui/src/lib/bringup); this is only the
//! transport, as the Nebulosa Debugger's src/core/instruments.cpp does it:
//!
//! - `/dev/usbtmcN` (Linux usbtmc driver): one write() is one USBTMC
//!   message, one read() its reply (a Siglent SPD3303C);
//! - `tcp:HOST[:PORT]`: SCPI over a raw socket, port 5025 by default
//!   (instruments with LAN);
//! - anything else: a serial port (a Korad KEL102's USB virtual COM port,
//!   115200 8N1).
//!
//! Every exchange blocks (an instrument wants a gap between commands), so
//! it runs on a blocking thread; each instrument has its own lock, so the
//! supply and the load don't wait for each other.

use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

enum Port {
    /// /dev/usbtmcN
    Tmc(std::fs::File),
    Serial(Box<dyn serialport::SerialPort>),
    Tcp(TcpStream),
}

impl Port {
    fn open(path: &str, baud: u32) -> Result<Port, String> {
        if let Some(addr) = path.strip_prefix("tcp:") {
            let addr = if addr.contains(':') { addr.to_string() } else { format!("{addr}:5025") };
            let sa = std::net::ToSocketAddrs::to_socket_addrs(&addr).map_err(|e| format!("{addr}: {e}"))?.next().ok_or(format!("{addr}: no address"))?;
            let s = TcpStream::connect_timeout(&sa, Duration::from_secs(3)).map_err(|e| format!("{addr}: {e}"))?;
            s.set_nodelay(true).ok();
            return Ok(Port::Tcp(s));
        }
        if path.contains("usbtmc") {
            let f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| format!("{path}: {e} (usbtmc driver loaded? udev rule for the instrument?)"))?;
            return Ok(Port::Tmc(f));
        }
        let p = serialport::new(path, baud)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| format!("{path}: {e}"))?;
        p.clear(serialport::ClearBuffer::All).ok();
        Ok(Port::Serial(p))
    }

    fn write_line(&mut self, line: &str) -> Result<(), String> {
        let l = format!("{line}\n");
        let r = match self {
            Port::Tmc(f) => f.write_all(l.as_bytes()),
            Port::Serial(p) => {
                // drop anything left from an earlier timeout
                p.clear(serialport::ClearBuffer::Input).ok();
                p.write_all(l.as_bytes())
            }
            Port::Tcp(s) => s.write_all(l.as_bytes()),
        };
        r.map_err(|e| format!("write: {e}"))
    }

    fn read_line(&mut self, timeout: Duration) -> Result<String, String> {
        match self {
            // the driver's own timeout (5 s by default); one read is the whole reply
            Port::Tmc(f) => {
                let mut b = [0u8; 512];
                let n = f.read(&mut b).map_err(|e| format!("read: {e}"))?;
                Ok(String::from_utf8_lossy(&b[..n]).trim().to_string())
            }
            Port::Serial(_) | Port::Tcp(_) => {
                let end = Instant::now() + timeout;
                let mut acc = Vec::new();
                let mut b = [0u8; 64];
                loop {
                    let left = end.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err("no answer (timeout)".into());
                    }
                    let n = match self {
                        Port::Serial(p) => {
                            p.set_timeout(left.min(Duration::from_millis(50))).ok();
                            match p.read(&mut b) {
                                Ok(n) => n,
                                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => 0,
                                Err(e) => return Err(format!("read: {e}")),
                            }
                        }
                        Port::Tcp(s) => {
                            s.set_read_timeout(Some(left.max(Duration::from_millis(1)))).ok();
                            match s.read(&mut b) {
                                Ok(0) => return Err("closed by the instrument".into()),
                                Ok(n) => n,
                                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => 0,
                                Err(e) => return Err(format!("read: {e}")),
                            }
                        }
                        Port::Tmc(_) => unreachable!(),
                    };
                    for &c in &b[..n] {
                        if c == b'\n' {
                            return Ok(String::from_utf8_lossy(&acc).trim().to_string());
                        }
                        acc.push(c);
                    }
                }
            }
        }
    }
}

/// The open instruments, by the UI's name for them ("psu", "load").
#[derive(Default)]
pub struct Instruments(Mutex<HashMap<String, Arc<Mutex<Port>>>>);

#[derive(Serialize)]
pub struct InstrumentPort {
    pub path: String,
    pub label: String,
}

impl Instruments {
    pub async fn open(&self, id: String, path: String, baud: u32) -> Result<(), String> {
        let port = tokio::task::spawn_blocking(move || Port::open(&path, if baud == 0 { 115_200 } else { baud })).await.map_err(|e| e.to_string())??;
        self.0.lock().unwrap().insert(id, Arc::new(Mutex::new(port)));
        Ok(())
    }

    pub async fn close(&self, id: String) -> Result<(), String> {
        self.0.lock().unwrap().remove(&id);
        Ok(())
    }

    /// Sends `line`; with `reply`, the line that comes back. Then waits `gap_ms`
    /// (the SPD3303C loses commands sent back to back).
    pub async fn io(&self, id: String, line: String, reply: bool, gap_ms: u64, timeout_ms: u64) -> Result<Option<String>, String> {
        let port = self.0.lock().unwrap().get(&id).cloned().ok_or(format!("{id}: not open"))?;
        tokio::task::spawn_blocking(move || {
            let mut p = port.lock().unwrap();
            p.write_line(&line)?;
            let out = if reply { Some(p.read_line(Duration::from_millis(timeout_ms.clamp(50, 10_000)))?) } else { None };
            std::thread::sleep(Duration::from_millis(gap_ms.min(1000)));
            Ok(out)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// Ports an instrument may be on: USBTMC devices and serial ports.
    pub async fn list(&self) -> Result<Vec<InstrumentPort>, String> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/dev") {
            let mut tmc: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("usbtmc")).collect();
            tmc.sort();
            out.extend(tmc.into_iter().map(|n| InstrumentPort { label: format!("/dev/{n} (USBTMC)"), path: format!("/dev/{n}") }));
        }
        if let Ok(s) = crate::serial::serial_list().await {
            out.extend(s.into_iter().map(|p| InstrumentPort { path: p.path, label: p.label }));
        }
        Ok(out)
    }
}

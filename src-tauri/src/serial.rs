//! A serial port: list, open with a full line config (baud, framing, flow,
//! RTS / DTR), bytes back on a channel, write, toggle the control lines, close.
//! One port at a time (the serial monitor and the side-channel target share it).

use serde::{Deserialize, Serialize};
use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::events::Events;

#[derive(Default)]
pub struct SerialState(Mutex<Option<Open>>);

struct Open {
    port: Box<dyn serialport::SerialPort>,
    stop: Arc<AtomicBool>,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Serialize)]
pub struct PortInfo {
    pub path: String,
    pub label: String,
}

pub async fn serial_list() -> Result<Vec<PortInfo>, String> {
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;
    Ok(ports
        .into_iter()
        .map(|p| {
            let label = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    let what = [u.manufacturer.as_deref(), u.product.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
                    format!("{} ({}{:04x}:{:04x})", p.port_name, if what.is_empty() { String::new() } else { format!("{what}, ") }, u.vid, u.pid)
                }
                _ => p.port_name.clone(),
            };
            PortInfo { path: p.port_name, label }
        })
        .collect())
}

/// The line settings for a port (the JS SerialConfig, framing part).
#[derive(Deserialize)]
pub struct PortCfg {
    pub baud: u32,
    /// 5, 6, 7 or 8
    pub data_bits: u8,
    /// "none" | "even" | "odd"
    pub parity: String,
    /// 1 or 2
    pub stop_bits: u8,
    /// "none" | "hardware" | "software"
    pub flow: String,
    pub rts: bool,
    pub dtr: bool,
}

fn apply(cfg: &PortCfg, b: serialport::SerialPortBuilder) -> serialport::SerialPortBuilder {
    b.data_bits(match cfg.data_bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        _ => DataBits::Eight,
    })
    .parity(match cfg.parity.as_str() {
        "even" => Parity::Even,
        "odd" => Parity::Odd,
        _ => Parity::None,
    })
    .stop_bits(if cfg.stop_bits == 2 { StopBits::Two } else { StopBits::One })
    .flow_control(match cfg.flow.as_str() {
        "hardware" => FlowControl::Hardware,
        "software" => FlowControl::Software,
        _ => FlowControl::None,
    })
}

impl SerialState {
/// Opens `path` with `cfg`; what it receives goes to `on_data`.
pub async fn serial_open(&self, path: String, cfg: PortCfg, on_data: Events) -> Result<(), String> {
    let mut port = apply(&cfg, serialport::new(&path, cfg.baud).timeout(Duration::from_millis(50))).open().map_err(|e| format!("{path}: {e}"))?;
    // control lines (ignored by ports that don't have them)
    let _ = port.write_request_to_send(cfg.rts);
    let _ = port.write_data_terminal_ready(cfg.dtr);
    let mut reader = port.try_clone().map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let quit = stop.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while !quit.load(Ordering::Relaxed) {
            match reader.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    if !on_data.bytes(buf[..n].to_vec()) {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    });
    *self.0.lock().unwrap() = Some(Open { port, stop });
    Ok(())
}

pub async fn serial_write(&self, data: Vec<u8>) -> Result<(), String> {
    let mut g = self.0.lock().unwrap();
    let o = g.as_mut().ok_or("no serial port open")?;
    o.port.write_all(&data).and_then(|_| o.port.flush()).map_err(|e| e.to_string())
}

/// Toggle the RTS / DTR control lines while open (e.g. to reset a board).
pub async fn serial_signals(&self, rts: bool, dtr: bool) -> Result<(), String> {
    let mut g = self.0.lock().unwrap();
    let o = g.as_mut().ok_or("no serial port open")?;
    o.port.write_request_to_send(rts).map_err(|e| e.to_string())?;
    o.port.write_data_terminal_ready(dtr).map_err(|e| e.to_string())
}

pub async fn serial_close(&self) -> Result<(), String> {
    self.0.lock().unwrap().take();
    Ok(())
}
}

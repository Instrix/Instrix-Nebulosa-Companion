//! A connection to one device: a writer for commands, reader tasks that feed
//! raw bytes to one dispatcher task, which runs the [`Pipeline`], hands
//! responses to the waiting request and sends point batches to the UI.

pub mod tcp;
pub mod usb;

use nbp_core::{msg, Calibration, Frame, Meta, Pipeline};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// What the UI asks to connect to.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    /// `id` from `list_usb`
    Usb { id: String },
    Tcp { host: String, port: Option<u16> },
}

/// Pushed to the UI on the connection's channel as JSON. Points go as raw
/// bytes (an ArrayBuffer in JS): `Batch::to_f64`, little-endian f64.
#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Event {
    Meta(Meta),
    Closed { reason: String },
}

pub type Events = Channel<InvokeResponseBody>;

fn send_json(ch: &Events, e: &Event) -> bool {
    match serde_json::to_string(e) {
        Ok(s) => ch.send(InvokeResponseBody::Json(s)).is_ok(),
        Err(_) => true,
    }
}

#[derive(Serialize)]
pub struct Reply {
    pub ty: u8,
    pub status: u8,
    pub payload: Vec<u8>,
}

/// From the reader tasks to the dispatcher.
pub(crate) enum Input {
    Bytes(usize, Vec<u8>),
    Closed(String),
}

enum Ctl {
    Bucket(u32),
    Calibration(Calibration),
}

pub(crate) enum Writer {
    Tcp(tokio::net::tcp::OwnedWriteHalf),
    Usb { iface: nusb::Interface, ep: u8 },
}

impl Writer {
    async fn write(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        match self {
            Writer::Tcp(w) => {
                use tokio::io::AsyncWriteExt;
                w.write_all(&bytes).await.map_err(|e| e.to_string())
            }
            Writer::Usb { iface, ep } => {
                iface.bulk_out(*ep, bytes).await.into_result().map(|_| ()).map_err(|e| e.to_string())
            }
        }
    }
}

type Pending = Arc<Mutex<Option<(u16, oneshot::Sender<Frame>)>>>;

pub struct Conn {
    writer: Writer,
    pending: Pending,
    seq: u16,
    ctl: mpsc::UnboundedSender<Ctl>,
    tasks: Vec<JoinHandle<()>>,
    pub description: String,
}

impl Conn {
    pub async fn open(target: Target, events: Events) -> Result<Conn, String> {
        let (in_tx, in_rx) = mpsc::channel::<Input>(256);
        let (writer, mut tasks, description) = match target {
            Target::Tcp { host, port } => tcp::open(&host, port.unwrap_or(msg::TCP_PORT), in_tx).await?,
            Target::Usb { id } => usb::open(&id, in_tx).await?,
        };
        let pending: Pending = Arc::default();
        let (ctl_tx, ctl_rx) = mpsc::unbounded_channel();
        tasks.push(tokio::spawn(dispatch(in_rx, ctl_rx, pending.clone(), events)));
        Ok(Conn { writer, pending, seq: 0, ctl: ctl_tx, tasks, description })
    }

    /// One command, its response. The protocol has one command in flight at
    /// a time; the caller holds `&mut self`, which ensures it.
    pub async fn request(&mut self, cmd: u8, payload: &[u8], timeout: Duration) -> Result<Reply, String> {
        if payload.len() > nbp_core::frame::MAX_PAYLOAD {
            return Err("payload too long".into());
        }
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        let (tx, rx) = oneshot::channel();
        *self.pending.lock().unwrap() = Some((seq, tx));
        self.writer.write(nbp_core::encode(cmd, seq, payload)).await?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(f)) => Ok(Reply { ty: f.ty, status: f.status, payload: f.payload }),
            Ok(Err(_)) => Err("connection closed".into()),
            Err(_) => {
                self.pending.lock().unwrap().take();
                Err(format!("no response to command 0x{cmd:02X}"))
            }
        }
    }

    pub fn set_bucket(&self, n: u32) {
        let _ = self.ctl.send(Ctl::Bucket(n));
    }

    pub fn set_calibration(&self, c: Calibration) {
        let _ = self.ctl.send(Ctl::Calibration(c));
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

async fn dispatch(
    mut rx: mpsc::Receiver<Input>,
    mut ctl: mpsc::UnboundedReceiver<Ctl>,
    pending: Pending,
    events: Events,
) {
    let mut p = Pipeline::new();
    let mut tick = tokio::time::interval(Duration::from_millis(40));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            m = rx.recv() => match m {
                Some(Input::Bytes(src, b)) => {
                    p.push(src, &b);
                    for f in p.take_responses() {
                        let mut slot = pending.lock().unwrap();
                        match slot.take() {
                            Some((seq, tx)) if seq == f.seq => { let _ = tx.send(f); }
                            other => {
                                log::warn!("unexpected response type 0x{:02X} seq {}", f.ty, f.seq);
                                *slot = other;
                            }
                        }
                    }
                }
                Some(Input::Closed(reason)) => {
                    send_json(&events, &Event::Closed { reason });
                    break;
                }
                None => break,
            },
            Some(c) = ctl.recv() => match c {
                Ctl::Bucket(n) => p.set_bucket(n),
                Ctl::Calibration(c) => p.set_calibration(c),
            },
            _ = tick.tick() => {
                if let Some(b) = p.take_batch() {
                    let bytes: Vec<u8> = b.to_f64().iter().flat_map(|x| x.to_le_bytes()).collect();
                    if events.send(InvokeResponseBody::Raw(bytes)).is_err()
                        || !send_json(&events, &Event::Meta(p.meta()))
                    {
                        break;
                    }
                }
            }
        }
    }
    // dropping the sender makes a waiting request fail at once
    pending.lock().unwrap().take();
}

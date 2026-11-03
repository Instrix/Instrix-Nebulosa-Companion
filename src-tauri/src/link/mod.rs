//! A connection to one device: a writer for commands, reader tasks that feed
//! raw bytes to one dispatcher task, which runs the [`Pipeline`] (shared with
//! the query commands), hands responses to the waiting request and sends
//! [`Meta`] to the UI. The UI reads samples by querying the pipeline.

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

/// Pushed to the UI on the connection's channel, as JSON.
#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Event {
    Meta(Meta),
    Closed { reason: String },
}

pub type Events = Channel<InvokeResponseBody>;

pub(crate) fn send_meta(ch: &Events, m: Meta) -> bool {
    send_json(ch, &Event::Meta(m))
}

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

/// The pipeline, shared by the dispatcher and the query commands.
pub type SharedPipeline = Arc<Mutex<Pipeline>>;

/// A writer thread's queue (bounded: a disk that can't keep up mustn't fill
/// RAM or hold the stream up); chunks that didn't fit are counted.
pub struct Sink {
    pub tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    pub dropped: Arc<std::sync::atomic::AtomicU64>,
}

impl Sink {
    pub fn send(&self, b: Vec<u8>) {
        if self.tx.try_send(b).is_err() {
            self.dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Where recording / log bytes go (a writer thread), while on.
pub type RecSink = Arc<Mutex<Option<Sink>>>;

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
    tasks: Vec<JoinHandle<()>>,
    pub pipe: SharedPipeline,
    pub rec: RecSink,
    /// where session-log file bytes go, while logging
    pub log: RecSink,
    pub description: String,
}

impl Conn {
    pub async fn open(target: Target, events: Events, buffer_samples: usize, cal: Calibration) -> Result<Conn, String> {
        let (in_tx, in_rx) = mpsc::channel::<Input>(256);
        let (writer, mut tasks, description) = match target {
            Target::Tcp { host, port } => tcp::open(&host, port.unwrap_or(msg::TCP_PORT), in_tx).await?,
            Target::Usb { id } => usb::open(&id, in_tx).await?,
        };
        let mut p = Pipeline::new();
        p.set_buffer_samples(buffer_samples);
        p.set_calibration(cal);
        let pipe: SharedPipeline = Arc::new(Mutex::new(p));
        let pending: Pending = Arc::default();
        let rec: RecSink = Arc::default();
        let log: RecSink = Arc::default();
        tasks.push(tokio::spawn(dispatch(in_rx, pipe.clone(), pending.clone(), rec.clone(), log.clone(), events)));
        Ok(Conn { writer, pending, seq: 0, tasks, pipe, rec, log, description })
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
    pipe: SharedPipeline,
    pending: Pending,
    rec: RecSink,
    log: RecSink,
    events: Events,
) {
    // recording bytes to the writer thread (whole blocks; all of it on `flush`)
    let forward = |p: &mut Pipeline, flush: bool| {
        if let Some(s) = rec.lock().unwrap().as_ref() {
            let b = p.take_recording(flush);
            if !b.is_empty() {
                s.send(b);
            }
        }
        if let Some(s) = log.lock().unwrap().as_ref() {
            let b = p.take_log();
            if !b.is_empty() {
                s.send(b);
            }
        }
    };
    let mut ticks = 0u32;
    let mut tick = tokio::time::interval(Duration::from_millis(40));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sent_version = u64::MAX;
    loop {
        tokio::select! {
            m = rx.recv() => match m {
                Some(Input::Bytes(src, b)) => {
                    let responses = {
                        let mut p = pipe.lock().unwrap();
                        p.push(src, &b);
                        forward(&mut p, false);
                        p.take_responses()
                    };
                    for f in responses {
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
            _ = tick.tick() => {
                ticks = ticks.wrapping_add(1);
                let meta = {
                    let mut p = pipe.lock().unwrap();
                    // about once a second, everything (slow rates fill blocks slowly)
                    forward(&mut p, ticks % 25 == 0);
                    p.meta()
                };
                if meta.version != sent_version {
                    sent_version = meta.version;
                    if !send_json(&events, &Event::Meta(meta)) {
                        break;
                    }
                }
            }
        }
    }
    // dropping the sender makes a waiting request fail at once
    pending.lock().unwrap().take();
}

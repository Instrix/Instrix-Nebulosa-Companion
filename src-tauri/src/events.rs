//! Messages pushed to the UI outside a command's reply (a connection's Meta,
//! a serial port's bytes). The desktop shells deliver them differently (a
//! Tauri channel, a WebSocket); the backend only sees this.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum Msg {
    /// a JSON document, already serialized
    Json(String),
    Bytes(Vec<u8>),
}

/// Where a stream of messages goes. `send` is false once the UI side is gone.
#[derive(Clone)]
pub struct Events(Arc<dyn Fn(Msg) -> bool + Send + Sync>);

impl Events {
    pub fn new(f: impl Fn(Msg) -> bool + Send + Sync + 'static) -> Self {
        Events(Arc::new(f))
    }
    /// nowhere (tests, tools)
    pub fn none() -> Self {
        Events::new(|_| true)
    }
    pub fn json(&self, s: String) -> bool {
        (self.0)(Msg::Json(s))
    }
    pub fn bytes(&self, b: Vec<u8>) -> bool {
        (self.0)(Msg::Bytes(b))
    }
    pub fn send(&self, m: Msg) -> bool {
        (self.0)(m)
    }
}

/// The UIs watching the open connection besides the one that opened it (remote
/// control: a browser mirroring the app). The connection's events go to all of
/// them; it lives on while any of them does.
#[derive(Clone, Default)]
pub struct Viewers(Arc<Mutex<Vec<Events>>>);

impl Viewers {
    pub fn add(&self, e: Events) {
        self.0.lock().unwrap().push(e);
    }
    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
    /// `primary`, and every viewer, get each message
    pub fn fan(&self, primary: Events) -> Events {
        let list = self.0.clone();
        Events::new(move |m| {
            let alive = primary.send(m.clone());
            let mut l = list.lock().unwrap();
            l.retain(|e| e.send(m.clone()));
            alive || !l.is_empty()
        })
    }
}

/// The UIs that keep each other's settings in step (remote control): what one
/// sends, the others get, as {"from": id, "msg": …}.
#[derive(Default)]
pub struct Peers {
    next: AtomicU64,
    list: Mutex<Vec<(u64, Events)>>,
}

impl Peers {
    pub fn join(&self, e: Events) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.list.lock().unwrap().push((id, e));
        id
    }
    pub fn leave(&self, id: u64) {
        self.list.lock().unwrap().retain(|(i, _)| *i != id);
    }
    pub fn send(&self, from: u64, msg: &serde_json::Value) {
        let text = serde_json::json!({ "from": from, "msg": msg }).to_string();
        self.list.lock().unwrap().retain(|(i, e)| *i == from || e.json(text.clone()));
    }
    pub fn count(&self) -> usize {
        self.list.lock().unwrap().len()
    }
}

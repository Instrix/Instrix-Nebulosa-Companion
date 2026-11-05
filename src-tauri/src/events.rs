//! Messages pushed to the UI outside a command's reply (a connection's Meta,
//! a serial port's bytes). The desktop shells deliver them differently (a
//! Tauri channel, a WebSocket); the backend only sees this.

use std::sync::Arc;

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
}

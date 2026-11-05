//! The backend behind a WebSocket, for a Chromium window (Linux: Electron, in
//! electron/). It serves the UI's files and one WebSocket, on 127.0.0.1 only,
//! and the WebSocket wants the token printed at start (other programs on the
//! machine, and web pages in a browser, can't drive the device).
//!
//! The protocol, small on purpose (ui/src/lib/ipc.ts is the other end):
//!   UI → backend, text:   {"id": n, "cmd": "query_lod", "args": {...}, "ch": c?}
//!     `ch`: a channel id for the commands that stream events (connect, …)
//!   backend → UI, text:   {"id": n, "ok": value} | {"id": n, "err": "…"}
//!                         {"ch": c, "msg": value}            (an event, JSON)
//!   backend → UI, binary: [1][id u32 LE][bytes]              (a reply in bytes)
//!                         [2][ch u32 LE][bytes]              (an event in bytes)

use crate::api::{dispatch, AppState, Out};
use crate::events::{Events, Msg};
use crate::serial::SerialState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::services::ServeDir;

struct Shared {
    app: AppState,
    serial: SerialState,
    token: String,
    origin: String,
}

fn token() -> String {
    let mut b = [0u8; 24];
    getrandom::fill(&mut b).expect("random token");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Serves `dist` and the WebSocket on 127.0.0.1:`port` (0: any free port).
/// Prints `NEBULOSA_BACKEND <url>` once listening: the URL the window opens.
pub async fn serve(dist: PathBuf, port: u16) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| format!("bind: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let origin = format!("http://127.0.0.1:{port}");
    let s = Arc::new(Shared { app: AppState::default(), serial: SerialState::default(), token: token(), origin: origin.clone() });
    println!("NEBULOSA_BACKEND {origin}/?nbk={}", s.token);
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let router = Router::new().route("/ws", get(upgrade)).fallback_service(ServeDir::new(dist)).with_state(s);
    axum::serve(listener, router).await.map_err(|e| e.to_string())
}

async fn upgrade(ws: WebSocketUpgrade, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, State(s): State<Arc<Shared>>) -> Response {
    let ok_token = q.get("t").is_some_and(|t| t.len() == s.token.len() && t.bytes().zip(s.token.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0);
    // a page from elsewhere (another site in a browser) can't connect, token or not
    let ok_origin = headers.get("origin").is_none_or(|o| o.as_bytes() == s.origin.as_bytes());
    if !ok_token || !ok_origin {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(512 << 20).on_upgrade(move |sock| session(sock, s))
}

fn framed(kind: u8, id: u32, b: Vec<u8>) -> Message {
    let mut v = Vec::with_capacity(5 + b.len());
    v.push(kind);
    v.extend_from_slice(&id.to_le_bytes());
    v.extend_from_slice(&b);
    Message::Binary(v.into())
}

async fn session(mut sock: WebSocket, s: Arc<Shared>) {
    let (otx, mut orx) = mpsc::unbounded_channel::<Message>();
    loop {
        tokio::select! {
            m = sock.recv() => {
                let Some(Ok(m)) = m else { break };
                let Message::Text(t) = m else { continue };
                let Ok(req) = serde_json::from_str::<serde_json::Value>(t.as_str()) else { continue };
                let id = req["id"].as_u64().unwrap_or(0) as u32;
                let cmd = req["cmd"].as_str().unwrap_or("").to_string();
                let args = req.get("args").cloned().unwrap_or(serde_json::Value::Null);
                let ev = req["ch"].as_u64().map(|ch| {
                    let ch = ch as u32;
                    let tx = otx.clone();
                    Events::new(move |m| {
                        tx.send(match m {
                            Msg::Json(j) => Message::Text(format!("{{\"ch\":{ch},\"msg\":{j}}}").into()),
                            Msg::Bytes(b) => framed(2, ch, b),
                        })
                        .is_ok()
                    })
                });
                let (s, tx) = (s.clone(), otx.clone());
                // commands run side by side, as Tauri runs them
                tokio::spawn(async move {
                    let reply = match dispatch(&s.app, &s.serial, &cmd, &args, ev).await {
                        Ok(Out::Json(v)) => Message::Text(serde_json::json!({ "id": id, "ok": v }).to_string().into()),
                        Ok(Out::Bytes(b)) => framed(1, id, b),
                        Err(e) => Message::Text(serde_json::json!({ "id": id, "err": e }).to_string().into()),
                    };
                    let _ = tx.send(reply);
                });
            }
            Some(out) = orx.recv() => {
                if sock.send(out).await.is_err() {
                    break;
                }
            }
        }
    }
}

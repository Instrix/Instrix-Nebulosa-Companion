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

use crate::api::AppState;
use crate::serial::SerialState;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

struct Shared {
    app: Arc<AppState>,
    serial: Arc<SerialState>,
    token: String,
    origin: String,
}

/// Serves `dist` and the WebSocket on 127.0.0.1:`port` (0: any free port).
/// Prints `NEBULOSA_BACKEND <url>` once listening: the URL the window opens.
pub async fn serve(dist: PathBuf, port: u16) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| format!("bind: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let origin = format!("http://127.0.0.1:{port}");
    let (app, serial) = (Arc::new(AppState::default()), Arc::new(SerialState::default()));
    // remote control (off until the window turns it on) shares this backend
    crate::remote::init(app.clone(), serial.clone(), crate::remote::Assets::Dir(dist.clone()));
    let s = Arc::new(Shared { app, serial, token: crate::remote::new_token(), origin: origin.clone() });
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
    let (app, serial) = (s.app.clone(), s.serial.clone());
    ws.max_message_size(512 << 20).on_upgrade(move |sock| crate::remote::session(sock, app, serial, false, None))
}

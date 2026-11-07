//! Remote control: the Companion in a browser, on a port of this computer (or
//! of the local network, when chosen), driving the same backend as the app's
//! window. Separate from the Linux shell's own server (server.rs: one window,
//! a private port).
//!
//!   GET  /                 the UI (the app's own files); open /?nbk=<token>&remote=1
//!   GET  /ws?t=<token>     the UI's WebSocket: the same protocol as server.rs
//!   GET|POST /api/v1/<action>[?t=<token>]   (or Authorization: Bearer <token>)
//!                          a JSON API for scripts; the app's window carries the
//!                          calls out (ui/src/lib/remote.ts lists the actions)
//!
//! The browser's UI joins what the window has open (`remote_attach`: the
//! connection's events fan out to it) and the UIs keep the instrument's
//! settings in step (`sync_*`, events.rs Peers). Every request needs the token.

use crate::events::Events;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

/// The UI that carries out the HTTP API's calls (the app's window), and the calls waiting for it.
#[derive(Default)]
pub struct ApiHost {
    ui: Mutex<Option<Events>>,
    next: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
}

impl ApiHost {
    pub fn host(&self, e: Events) {
        *self.ui.lock().unwrap() = Some(e);
    }

    pub fn reply(&self, id: u64, ok: Option<Value>, err: Option<String>) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(match err {
                Some(e) => Err(e),
                None => Ok(ok.unwrap_or(Value::Null)),
            });
        }
    }

    pub async fn call(&self, action: &str, args: Value, timeout: Duration) -> Result<Value, String> {
        let ui = self.ui.lock().unwrap().clone().ok_or("the Companion's window isn't open")?;
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if !ui.json(serde_json::json!({ "id": id, "action": action, "args": args }).to_string()) {
            self.pending.lock().unwrap().remove(&id);
            *self.ui.lock().unwrap() = None;
            return Err("the Companion's window isn't open".into());
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("no answer".into()),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err("the Companion's window did not answer in time".into())
            }
        }
    }
}

#[cfg(any(feature = "server", feature = "tauri-app"))]
pub use http::*;

#[cfg(any(feature = "server", feature = "tauri-app"))]
mod http {
    use super::*;
    use crate::api::{dispatch, AppState, Out};
    use crate::events::Msg;
    use crate::serial::SerialState;
    use axum::body::Body;
    use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
    use axum::extract::{Path, Query, State};
    use axum::http::{header, HeaderMap, StatusCode, Uri};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{any, get};
    use axum::Router;
    use std::path::PathBuf;
    use std::sync::{Arc, OnceLock};
    use tokio::sync::{mpsc, watch};

    /// a file of the app's by its path: its bytes and type
    pub type AssetFn = Box<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

    /// Where the UI's files come from: a folder (the Linux backend's dist), or
    /// the app itself (Tauri embeds them).
    pub enum Assets {
        Dir(PathBuf),
        Embedded(AssetFn),
    }

    struct Ctx {
        app: Arc<AppState>,
        serial: Arc<SerialState>,
        assets: Assets,
    }

    static CTX: OnceLock<Ctx> = OnceLock::new();
    static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

    struct Running {
        port: u16,
        lan: bool,
        token: String,
        /// true: the server and its sessions end
        stop: watch::Sender<bool>,
    }

    /// The shell's backend, for the remote server to share (once, at start).
    pub fn init(app: Arc<AppState>, serial: Arc<SerialState>, assets: Assets) {
        let _ = CTX.set(Ctx { app, serial, assets });
    }

    #[derive(serde::Serialize, Default)]
    pub struct RemoteStatus {
        pub running: bool,
        pub port: u16,
        pub lan: bool,
        pub token: String,
        /// the addresses to open, the token in them
        pub urls: Vec<String>,
        /// UIs connected (the window's own included)
        pub peers: usize,
    }

    pub fn new_token() -> String {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).expect("random token");
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// this computer's address on the local network (the route out; nothing is sent)
    fn lan_ip() -> Option<std::net::IpAddr> {
        let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("192.0.2.1:9").ok()?;
        s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
    }

    pub fn status() -> RemoteStatus {
        let r = RUNNING.lock().unwrap();
        let peers = CTX.get().map_or(0, |c| c.app.peers.count());
        match r.as_ref() {
            None => RemoteStatus { peers, ..Default::default() },
            Some(r) => {
                let link = |host: String| format!("http://{host}:{}/?nbk={}&remote=1", r.port, r.token);
                let mut urls = vec![link("127.0.0.1".into())];
                if r.lan {
                    if let Some(ip) = lan_ip() {
                        urls.push(link(ip.to_string()));
                    }
                }
                RemoteStatus { running: true, port: r.port, lan: r.lan, token: r.token.clone(), urls, peers }
            }
        }
    }

    pub fn stop() -> RemoteStatus {
        if let Some(r) = RUNNING.lock().unwrap().take() {
            let _ = r.stop.send(true);
        }
        status()
    }

    /// Starts (or restarts) the server: `port` (0: any free one), on this computer
    /// only or (`lan`) on every interface; `token`: the one to keep, else a new one.
    /// (boxed: its server's sessions dispatch commands, this among them)
    pub fn start(port: u16, lan: bool, token: Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RemoteStatus, String>> + Send>> {
        Box::pin(start_now(port, lan, token))
    }

    async fn start_now(port: u16, lan: bool, token: Option<String>) -> Result<RemoteStatus, String> {
        if CTX.get().is_none() {
            return Err("remote control isn't available in this build".into());
        }
        stop();
        let token = token.filter(|t| t.len() >= 16 && t.bytes().all(|b| b.is_ascii_alphanumeric())).unwrap_or_else(new_token);
        let bind = if lan { "0.0.0.0" } else { "127.0.0.1" };
        let listener = tokio::net::TcpListener::bind((bind, port)).await.map_err(|e| format!("port {port}: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let (tx, rx) = watch::channel(false);
        let shared = Arc::new(Server { token: token.clone(), lan, stop: rx.clone() });
        let router = Router::new()
            .route("/ws", get(upgrade))
            .route("/api/v1/{action}", any(api))
            .fallback(get(asset))
            .with_state(shared);
        let mut done = rx;
        tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { ended(&mut done).await })
                .await;
        });
        *RUNNING.lock().unwrap() = Some(Running { port, lan, token, stop: tx });
        Ok(status())
    }

    struct Server {
        token: String,
        lan: bool,
        stop: watch::Receiver<bool>,
    }

    /// resolves once the flag is true (never, when its sender is gone)
    async fn ended(rx: &mut watch::Receiver<bool>) {
        loop {
            if *rx.borrow_and_update() {
                return;
            }
            if rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }

    fn same(a: &str, b: &str) -> bool {
        a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }

    impl Server {
        /// the Host is this computer, unless the network is allowed (no DNS rebinding)
        fn host_ok(&self, h: &HeaderMap) -> bool {
            if self.lan {
                return true;
            }
            let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
            let name = host.rsplit_once(':').map_or(host, |(n, _)| n);
            matches!(name, "127.0.0.1" | "localhost" | "[::1]")
        }
        /// a browser page from elsewhere can't use it (its Origin isn't this server)
        fn origin_ok(&self, h: &HeaderMap) -> bool {
            let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
            h.get(header::ORIGIN).is_none_or(|o| o.to_str().is_ok_and(|o| o == format!("http://{host}")))
        }
        fn token_ok(&self, h: &HeaderMap, q: &HashMap<String, String>) -> bool {
            let bearer = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
            bearer.or(q.get("t").map(String::as_str)).is_some_and(|t| same(t.trim(), &self.token))
        }
    }

    async fn upgrade(ws: WebSocketUpgrade, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, State(s): State<Arc<Server>>) -> Response {
        if !s.host_ok(&headers) || !s.origin_ok(&headers) || !s.token_ok(&headers, &q) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let Some(c) = CTX.get() else { return StatusCode::SERVICE_UNAVAILABLE.into_response() };
        let stop = s.stop.clone();
        ws.max_message_size(512 << 20).on_upgrade(move |sock| session(sock, c.app.clone(), c.serial.clone(), true, Some(stop)))
    }

    /// commands a remote UI may not send: the remote server's own controls, and taking over the API
    const WINDOW_ONLY: &[&str] = &["remote_start", "remote_stop", "api_host"];

    fn framed(kind: u8, id: u32, b: Vec<u8>) -> Message {
        let mut v = Vec::with_capacity(5 + b.len());
        v.push(kind);
        v.extend_from_slice(&id.to_le_bytes());
        v.extend_from_slice(&b);
        Message::Binary(v.into())
    }

    /// One UI on a WebSocket (server.rs's protocol). `remote`: a browser through the remote
    /// server (not the window's own); `stop`: ends it when the server stops.
    pub async fn session(mut sock: WebSocket, app: Arc<AppState>, serial: Arc<SerialState>, remote: bool, stop: Option<watch::Receiver<bool>>) {
        let (otx, mut orx) = mpsc::unbounded_channel::<Message>();
        let mut stop = stop.unwrap_or_else(|| watch::channel(false).1);
        loop {
            tokio::select! {
                m = sock.recv() => {
                    let Some(Ok(m)) = m else { break };
                    let Message::Text(t) = m else { continue };
                    let Ok(req) = serde_json::from_str::<Value>(t.as_str()) else { continue };
                    let id = req["id"].as_u64().unwrap_or(0) as u32;
                    let cmd = req["cmd"].as_str().unwrap_or("").to_string();
                    let args = req.get("args").cloned().unwrap_or(Value::Null);
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
                    let (app, serial, tx) = (app.clone(), serial.clone(), otx.clone());
                    // commands run side by side, as Tauri runs them
                    tokio::spawn(async move {
                        let r = if remote && WINDOW_ONLY.contains(&cmd.as_str()) {
                            Err(format!("{cmd}: only from the Companion's window"))
                        } else {
                            dispatch(&app, &serial, &cmd, &args, ev).await
                        };
                        let reply = match r {
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
                _ = ended(&mut stop) => break,
            }
        }
    }

    async fn api(Path(action): Path<String>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, State(s): State<Arc<Server>>, body: axum::body::Bytes) -> Response {
        if !s.host_ok(&headers) || !s.origin_ok(&headers) {
            return StatusCode::FORBIDDEN.into_response();
        }
        if !s.token_ok(&headers, &q) {
            return (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({ "error": "the token is missing or wrong" }))).into_response();
        }
        let Some(c) = CTX.get() else { return StatusCode::SERVICE_UNAVAILABLE.into_response() };
        // arguments: the JSON body, else the query (numbers and booleans as such)
        let args = if body.is_empty() {
            Value::Object(
                q.into_iter()
                    .filter(|(k, _)| k != "t")
                    .map(|(k, v)| (k, serde_json::from_str::<Value>(&v).unwrap_or(Value::String(v))))
                    .collect(),
            )
        } else {
            match serde_json::from_slice::<Value>(&body) {
                Ok(v) => v,
                Err(e) => return (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({ "error": format!("body: {e}") }))).into_response(),
            }
        };
        match c.app.api.call(&action, args, Duration::from_secs(30)).await {
            Ok(v) => axum::Json(serde_json::json!({ "ok": v })).into_response(),
            Err(e) => (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({ "error": e }))).into_response(),
        }
    }

    pub fn mime(path: &str) -> &'static str {
        match path.rsplit('.').next().unwrap_or("") {
            "html" => "text/html; charset=utf-8",
            "js" | "mjs" => "text/javascript",
            "css" => "text/css",
            "wasm" => "application/wasm",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "ico" => "image/x-icon",
            "json" => "application/json",
            "woff2" => "font/woff2",
            "py" | "txt" => "text/plain; charset=utf-8",
            _ => "application/octet-stream",
        }
    }

    async fn asset(uri: Uri, headers: HeaderMap, State(s): State<Arc<Server>>) -> Response {
        if !s.host_ok(&headers) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let Some(c) = CTX.get() else { return StatusCode::SERVICE_UNAVAILABLE.into_response() };
        let path = uri.path().trim_start_matches('/');
        if path.split('/').any(|p| p == "..") {
            return StatusCode::NOT_FOUND.into_response();
        }
        let path = if path.is_empty() { "index.html" } else { path };
        let found = match &c.assets {
            Assets::Embedded(get) => get(path),
            Assets::Dir(d) => std::fs::read(d.join(path)).ok().map(|b| (b, mime(path).to_string())),
        };
        match found {
            Some((bytes, ty)) => ([(header::CONTENT_TYPE, ty), (header::CACHE_CONTROL, "no-cache".into())], Body::from(bytes)).into_response(),
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        }
    }
}

//! The Tauri shell (Windows, macOS): a command per backend method, events
//! on Tauri channels. Linux runs the same backend behind a WebSocket
//! (server.rs) in a Chromium window instead.

use crate::api::{AppState, DiskStatus};
use crate::events::Events;
use crate::link::{self, Reply, Target};
use crate::serial::SerialState;
use crate::{discovery, files};
use nbp_core::buffer::SpanStats;
use nbp_core::Calibration;
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use std::sync::Arc;
use tauri::{Manager, State};

fn link_events(ch: Channel<InvokeResponseBody>) -> Events {
    Events::new(move |m| {
        ch.send(match m {
            crate::events::Msg::Json(s) => InvokeResponseBody::Json(s),
            crate::events::Msg::Bytes(b) => InvokeResponseBody::Raw(b),
        })
        .is_ok()
    })
}

fn bytes_events(ch: Channel<Vec<u8>>) -> Events {
    Events::new(move |m| match m {
        crate::events::Msg::Bytes(b) => ch.send(b).is_ok(),
        crate::events::Msg::Json(_) => true,
    })
}

/// A web page (a release, a download) in the system's browser: https only.
#[tauri::command]
async fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") || url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"') {
        return Err("not an https address".into());
    }
    #[cfg(target_os = "windows")]
    let mut c = {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", &url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut c = {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut c = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    c.spawn().map(|_| ()).map_err(|e| format!("could not open the browser: {e}"))
}

#[tauri::command]
async fn list_usb(state: State<'_, Arc<AppState>>) -> Result<Vec<link::usb::UsbDev>, String> {
    state.list_usb().await
}

#[tauri::command]
async fn discover(state: State<'_, Arc<AppState>>, timeout_ms: Option<u64>) -> Result<Vec<discovery::NetDev>, String> {
    state.discover(timeout_ms).await
}

#[tauri::command]
async fn connect(state: State<'_, Arc<AppState>>, target: Target, events: Channel<InvokeResponseBody>) -> Result<String, String> {
    state.connect(target, link_events(events)).await
}

#[tauri::command]
async fn disconnect(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.disconnect().await
}

#[tauri::command]
async fn recordings_dir(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    state.recordings_dir()
}

#[tauri::command]
async fn list_recordings(state: State<'_, Arc<AppState>>) -> Result<Vec<files::RecFile>, String> {
    state.list_recordings()
}

#[tauri::command]
async fn start_recording(state: State<'_, Arc<AppState>>, header: String, from: Option<f64>) -> Result<String, String> {
    state.start_recording(header, from).await
}

#[tauri::command]
async fn stop_recording(state: State<'_, Arc<AppState>>) -> Result<Option<files::RecordingDone>, String> {
    state.stop_recording().await
}

#[tauri::command]
async fn start_logging(state: State<'_, Arc<AppState>>, header: String, from: Option<f64>) -> Result<String, String> {
    state.start_logging(header, from).await
}

#[tauri::command]
async fn stop_logging(state: State<'_, Arc<AppState>>) -> Result<Option<files::LoggingDone>, String> {
    state.stop_logging().await
}

#[tauri::command]
async fn export_preview(state: State<'_, Arc<AppState>>, cfg: nbp_core::export::ExportCfg, wall_offset: f64, rows: usize) -> Result<String, String> {
    state.export_preview(cfg, wall_offset, rows).await
}

#[tauri::command]
async fn start_export(state: State<'_, Arc<AppState>>, cfg: nbp_core::export::ExportCfg, wall_offset: f64, header: String, from: Option<f64>) -> Result<String, String> {
    state.start_export(cfg, wall_offset, header, from).await
}

#[tauri::command]
async fn stop_export(state: State<'_, Arc<AppState>>) -> Result<Option<files::ExportDone>, String> {
    state.stop_export().await
}

#[tauri::command]
async fn set_log_interval(state: State<'_, Arc<AppState>>, interval: f64) -> Result<(), String> {
    state.set_log_interval(interval).await
}

#[tauri::command]
async fn clear_session(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.clear_session().await
}

#[tauri::command]
async fn session_lod(state: State<'_, Arc<AppState>>, t0: f64, t1: f64, bins: usize) -> Result<Response, String> {
    state.session_lod(t0, t1, bins).await.map(Response::new)
}

#[tauri::command]
async fn session_stats(state: State<'_, Arc<AppState>>, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
    state.session_stats(t0, t1).await
}

#[tauri::command]
async fn disk_status(state: State<'_, Arc<AppState>>) -> Result<DiskStatus, String> {
    state.disk_status()
}

#[tauri::command]
async fn open_log(state: State<'_, Arc<AppState>>, path: String, events: Channel<InvokeResponseBody>) -> Result<String, String> {
    state.open_log(path, link_events(events)).await
}

#[tauri::command]
async fn open_recording(state: State<'_, Arc<AppState>>, path: String, events: Channel<InvokeResponseBody>) -> Result<String, String> {
    state.open_recording(path, link_events(events)).await
}

#[tauri::command]
async fn export_csv(state: State<'_, Arc<AppState>>, t0: f64, t1: f64, path: String) -> Result<u64, String> {
    state.export_csv(t0, t1, path).await
}

#[tauri::command]
async fn count_samples(state: State<'_, Arc<AppState>>, t0: f64, t1: f64) -> Result<u64, String> {
    state.count_samples(t0, t1).await
}

#[tauri::command]
async fn save_bytes(state: State<'_, Arc<AppState>>, path: String, bytes: Vec<u8>) -> Result<(), String> {
    state.save_bytes(path, bytes).await
}

#[tauri::command]
async fn request(state: State<'_, Arc<AppState>>, cmd: u8, payload: Vec<u8>, timeout_ms: Option<u64>) -> Result<Reply, String> {
    state.request(cmd, payload, timeout_ms).await
}

#[tauri::command]
async fn set_buffer(state: State<'_, Arc<AppState>>, samples: usize) -> Result<(), String> {
    state.set_buffer(samples).await
}

#[tauri::command]
async fn set_calibration(state: State<'_, Arc<AppState>>, cal: Calibration) -> Result<(), String> {
    state.set_calibration(cal).await
}

#[tauri::command]
async fn set_trigger(state: State<'_, Arc<AppState>>, cfg: nbp_core::trigger::TriggerCfg) -> Result<(), String> {
    state.set_trigger(cfg).await
}

#[tauri::command]
async fn set_math(state: State<'_, Arc<AppState>>, expr: Option<String>) -> Result<(), String> {
    state.set_math(expr).await
}

#[tauri::command]
async fn analyze(state: State<'_, Arc<AppState>>, t0: f64, t1: f64, req: nbp_core::analysis::Request) -> Result<nbp_core::analysis::Result, String> {
    state.analyze(t0, t1, req).await
}

#[tauri::command]
async fn sca_new(state: State<'_, Arc<AppState>>, cfg: nbp_core::sca::Config) -> Result<(), String> {
    state.sca_new(cfg).await
}

#[tauri::command]
async fn sca_add(state: State<'_, Arc<AppState>>, t0: f64, t1: f64, meta: nbp_core::sca::TraceMeta) -> Result<usize, String> {
    state.sca_add(t0, t1, meta).await
}

#[tauri::command]
async fn sca_info(state: State<'_, Arc<AppState>>) -> Result<Option<nbp_core::sca::Info>, String> {
    state.sca_info().await
}

#[tauri::command]
async fn sca_align(state: State<'_, Arc<AppState>>, a: usize, b: usize, max_shift: usize) -> Result<nbp_core::sca::AlignResult, String> {
    state.sca_align(a, b, max_shift).await
}

#[tauri::command]
async fn sca_clear_align(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.sca_clear_align().await
}

#[tauri::command]
async fn sca_query(state: State<'_, Arc<AppState>>, req: nbp_core::sca::Request) -> Result<nbp_core::sca::Result, String> {
    state.sca_query(req).await
}

#[tauri::command]
async fn sca_meta(state: State<'_, Arc<AppState>>) -> Result<Vec<nbp_core::sca::TraceMeta>, String> {
    state.sca_meta().await
}

#[tauri::command]
async fn sca_export(state: State<'_, Arc<AppState>>) -> Result<Response, String> {
    state.sca_export().await.map(Response::new)
}

#[tauri::command]
async fn sca_save(state: State<'_, Arc<AppState>>) -> Result<Response, String> {
    state.sca_save().await.map(Response::new)
}

#[tauri::command]
async fn sca_load(state: State<'_, Arc<AppState>>, bytes: Vec<u8>) -> Result<nbp_core::sca::Info, String> {
    state.sca_load(bytes).await
}

#[tauri::command]
async fn probe_arm(state: State<'_, Arc<AppState>>, range: i32) -> Result<(), String> {
    state.probe_arm(range).await
}

#[tauri::command]
async fn probe_take(state: State<'_, Arc<AppState>>) -> Result<Option<nbp_core::debug::ProbeResult>, String> {
    state.probe_take().await
}

#[tauri::command]
async fn inst_open(state: State<'_, Arc<AppState>>, id: String, path: String, baud: u32) -> Result<(), String> {
    state.instruments.open(id, path, baud).await
}

#[tauri::command]
async fn inst_close(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.instruments.close(id).await
}

#[tauri::command]
async fn inst_io(state: State<'_, Arc<AppState>>, id: String, line: String, reply: bool, gap_ms: u64, timeout_ms: u64) -> Result<Option<String>, String> {
    state.instruments.io(id, line, reply, gap_ms, timeout_ms).await
}

#[tauri::command]
async fn inst_list(state: State<'_, Arc<AppState>>) -> Result<Vec<crate::instruments::InstrumentPort>, String> {
    state.instruments.list().await
}

#[tauri::command]
async fn query_events(state: State<'_, Arc<AppState>>, since: u64, max: usize) -> Result<Vec<nbp_core::trigger::Event>, String> {
    state.query_events(since, max).await
}

#[tauri::command]
async fn clear_events(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.clear_events().await
}

#[tauri::command]
async fn inject_events(state: State<'_, Arc<AppState>>, events: Vec<nbp_core::trigger::Event>) -> Result<(), String> {
    state.inject_events(events).await
}

#[tauri::command]
async fn script_load(state: State<'_, Arc<AppState>>, python: String, code: String) -> Result<crate::script::ScriptInfo, String> {
    state.script_load(python, code).await
}

#[tauri::command]
async fn script_scan(state: State<'_, Arc<AppState>>, req: crate::api::ScanReq) -> Result<crate::api::ScanResult, String> {
    state.script_scan(req).await
}

#[tauri::command]
async fn script_check(state: State<'_, Arc<AppState>>, req: crate::api::ScanReq) -> Result<crate::api::CheckResult, String> {
    state.script_check(req).await
}

#[tauri::command]
async fn script_reset(state: State<'_, Arc<AppState>>, python: String, code: String) -> Result<(), String> {
    state.script_reset(python, code).await
}

#[tauri::command]
async fn script_stop(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.script_stop().await
}

#[tauri::command]
async fn query_lod(state: State<'_, Arc<AppState>>, t0: f64, t1: f64, bins: usize) -> Result<Response, String> {
    state.query_lod(t0, t1, bins).await.map(Response::new)
}

#[tauri::command]
async fn query_stats(state: State<'_, Arc<AppState>>, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
    state.query_stats(t0, t1).await
}

#[tauri::command]
async fn query_value(state: State<'_, Arc<AppState>>, t: f64) -> Result<Option<(f64, f64, u8, f64)>, String> {
    state.query_value(t).await
}

#[tauri::command]
async fn serial_open(state: State<'_, Arc<SerialState>>, path: String, cfg: crate::serial::PortCfg, on_data: Channel<Vec<u8>>) -> Result<(), String> {
    state.serial_open(path, cfg, bytes_events(on_data)).await
}

#[tauri::command]
async fn serial_write(state: State<'_, Arc<SerialState>>, data: Vec<u8>) -> Result<(), String> {
    state.serial_write(data).await
}

#[tauri::command]
async fn serial_signals(state: State<'_, Arc<SerialState>>, rts: bool, dtr: bool) -> Result<(), String> {
    state.serial_signals(rts, dtr).await
}

#[tauri::command]
async fn serial_close(state: State<'_, Arc<SerialState>>) -> Result<(), String> {
    state.serial_close().await
}

#[tauri::command]
async fn serial_list() -> Result<Vec<crate::serial::PortInfo>, String> {
    crate::serial::serial_list().await
}

// ---- remote control (remote.rs) ------------------------------------------------------

#[tauri::command]
async fn remote_attach(state: State<'_, Arc<AppState>>, events: Channel<InvokeResponseBody>) -> Result<Option<crate::api::OpenInfo>, String> {
    state.attach(link_events(events)).await
}

#[tauri::command]
fn sync_join(state: State<'_, Arc<AppState>>, events: Channel<InvokeResponseBody>) -> u64 {
    state.peers.join(link_events(events))
}

#[tauri::command]
fn sync_leave(state: State<'_, Arc<AppState>>, id: u64) {
    state.peers.leave(id)
}

#[tauri::command]
fn sync_send(state: State<'_, Arc<AppState>>, from: u64, msg: serde_json::Value) {
    state.peers.send(from, &msg)
}

#[tauri::command]
fn api_host(state: State<'_, Arc<AppState>>, events: Channel<InvokeResponseBody>) {
    state.api.host(link_events(events))
}

#[tauri::command]
fn api_reply(state: State<'_, Arc<AppState>>, id: u64, ok: Option<serde_json::Value>, err: Option<String>) {
    state.api.reply(id, ok, err)
}

#[tauri::command]
async fn remote_start(port: u16, lan: bool, token: Option<String>) -> Result<crate::remote::RemoteStatus, String> {
    crate::remote::start(port, lan, token).await
}

#[tauri::command]
fn remote_stop() -> crate::remote::RemoteStatus {
    crate::remote::stop()
}

#[tauri::command]
fn remote_status() -> crate::remote::RemoteStatus {
    crate::remote::status()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (st, ser) = (Arc::new(AppState::default()), Arc::new(SerialState::default()));
            // the UI's files for remote control: the app's own (embedded); in `tauri dev`, the dist folder
            let res = app.asset_resolver();
            let dist = std::env::current_dir().unwrap_or_default().join("../dist");
            crate::remote::init(
                st.clone(),
                ser.clone(),
                crate::remote::Assets::Embedded(Box::new(move |p| {
                    res.get(p.to_string()).map(|a| (a.bytes, a.mime_type)).or_else(|| std::fs::read(dist.join(p)).ok().map(|b| (b, crate::remote::mime(p).to_string())))
                })),
            );
            app.manage(st);
            app.manage(ser);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_usb, discover, connect, disconnect, recordings_dir, list_recordings, start_recording, stop_recording, start_logging, stop_logging, export_preview, start_export, stop_export, set_log_interval, clear_session, session_lod, session_stats, disk_status, open_log, open_recording, export_csv, count_samples, save_bytes, request, set_buffer, set_calibration, set_trigger, set_math, analyze, sca_new, sca_add, sca_info, sca_align, sca_clear_align, sca_query, sca_meta, sca_export, sca_save, sca_load, query_events, clear_events, inject_events, script_load, script_scan, script_check, script_reset, script_stop, query_lod, query_stats, query_value, serial_open, serial_write, serial_signals, serial_close, serial_list, probe_arm, probe_take, inst_open, inst_close, inst_io, inst_list, open_url, remote_attach, sync_join, sync_leave, sync_send, api_host, api_reply, remote_start, remote_stop, remote_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

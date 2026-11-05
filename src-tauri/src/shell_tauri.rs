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
use tauri::State;

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

#[tauri::command]
async fn list_usb(state: State<'_, AppState>) -> Result<Vec<link::usb::UsbDev>, String> {
    state.list_usb().await
}

#[tauri::command]
async fn discover(state: State<'_, AppState>, timeout_ms: Option<u64>) -> Result<Vec<discovery::NetDev>, String> {
    state.discover(timeout_ms).await
}

#[tauri::command]
async fn connect(state: State<'_, AppState>, target: Target, events: Channel<InvokeResponseBody>) -> Result<String, String> {
    state.connect(target, link_events(events)).await
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    state.disconnect().await
}

#[tauri::command]
async fn recordings_dir(state: State<'_, AppState>) -> Result<String, String> {
    state.recordings_dir()
}

#[tauri::command]
async fn list_recordings(state: State<'_, AppState>) -> Result<Vec<files::RecFile>, String> {
    state.list_recordings()
}

#[tauri::command]
async fn start_recording(state: State<'_, AppState>, header: String, from: Option<f64>) -> Result<String, String> {
    state.start_recording(header, from).await
}

#[tauri::command]
async fn stop_recording(state: State<'_, AppState>) -> Result<Option<files::RecordingDone>, String> {
    state.stop_recording().await
}

#[tauri::command]
async fn start_logging(state: State<'_, AppState>, header: String, from: Option<f64>) -> Result<String, String> {
    state.start_logging(header, from).await
}

#[tauri::command]
async fn stop_logging(state: State<'_, AppState>) -> Result<Option<files::LoggingDone>, String> {
    state.stop_logging().await
}

#[tauri::command]
async fn export_preview(state: State<'_, AppState>, cfg: nbp_core::export::ExportCfg, wall_offset: f64, rows: usize) -> Result<String, String> {
    state.export_preview(cfg, wall_offset, rows).await
}

#[tauri::command]
async fn start_export(state: State<'_, AppState>, cfg: nbp_core::export::ExportCfg, wall_offset: f64, header: String, from: Option<f64>) -> Result<String, String> {
    state.start_export(cfg, wall_offset, header, from).await
}

#[tauri::command]
async fn stop_export(state: State<'_, AppState>) -> Result<Option<files::ExportDone>, String> {
    state.stop_export().await
}

#[tauri::command]
async fn set_log_interval(state: State<'_, AppState>, interval: f64) -> Result<(), String> {
    state.set_log_interval(interval).await
}

#[tauri::command]
async fn clear_session(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_session().await
}

#[tauri::command]
async fn session_lod(state: State<'_, AppState>, t0: f64, t1: f64, bins: usize) -> Result<Response, String> {
    state.session_lod(t0, t1, bins).await.map(Response::new)
}

#[tauri::command]
async fn session_stats(state: State<'_, AppState>, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
    state.session_stats(t0, t1).await
}

#[tauri::command]
async fn disk_status(state: State<'_, AppState>) -> Result<DiskStatus, String> {
    state.disk_status()
}

#[tauri::command]
async fn open_log(state: State<'_, AppState>, path: String, events: Channel<InvokeResponseBody>) -> Result<String, String> {
    state.open_log(path, link_events(events)).await
}

#[tauri::command]
async fn open_recording(state: State<'_, AppState>, path: String, events: Channel<InvokeResponseBody>) -> Result<String, String> {
    state.open_recording(path, link_events(events)).await
}

#[tauri::command]
async fn export_csv(state: State<'_, AppState>, t0: f64, t1: f64, path: String) -> Result<u64, String> {
    state.export_csv(t0, t1, path).await
}

#[tauri::command]
async fn count_samples(state: State<'_, AppState>, t0: f64, t1: f64) -> Result<u64, String> {
    state.count_samples(t0, t1).await
}

#[tauri::command]
async fn save_bytes(state: State<'_, AppState>, path: String, bytes: Vec<u8>) -> Result<(), String> {
    state.save_bytes(path, bytes).await
}

#[tauri::command]
async fn request(state: State<'_, AppState>, cmd: u8, payload: Vec<u8>, timeout_ms: Option<u64>) -> Result<Reply, String> {
    state.request(cmd, payload, timeout_ms).await
}

#[tauri::command]
async fn set_buffer(state: State<'_, AppState>, samples: usize) -> Result<(), String> {
    state.set_buffer(samples).await
}

#[tauri::command]
async fn set_calibration(state: State<'_, AppState>, cal: Calibration) -> Result<(), String> {
    state.set_calibration(cal).await
}

#[tauri::command]
async fn set_trigger(state: State<'_, AppState>, cfg: nbp_core::trigger::TriggerCfg) -> Result<(), String> {
    state.set_trigger(cfg).await
}

#[tauri::command]
async fn set_math(state: State<'_, AppState>, expr: Option<String>) -> Result<(), String> {
    state.set_math(expr).await
}

#[tauri::command]
async fn analyze(state: State<'_, AppState>, t0: f64, t1: f64, req: nbp_core::analysis::Request) -> Result<nbp_core::analysis::Result, String> {
    state.analyze(t0, t1, req).await
}

#[tauri::command]
async fn sca_new(state: State<'_, AppState>, cfg: nbp_core::sca::Config) -> Result<(), String> {
    state.sca_new(cfg).await
}

#[tauri::command]
async fn sca_add(state: State<'_, AppState>, t0: f64, t1: f64, meta: nbp_core::sca::TraceMeta) -> Result<usize, String> {
    state.sca_add(t0, t1, meta).await
}

#[tauri::command]
async fn sca_info(state: State<'_, AppState>) -> Result<Option<nbp_core::sca::Info>, String> {
    state.sca_info().await
}

#[tauri::command]
async fn sca_align(state: State<'_, AppState>, a: usize, b: usize, max_shift: usize) -> Result<nbp_core::sca::AlignResult, String> {
    state.sca_align(a, b, max_shift).await
}

#[tauri::command]
async fn sca_clear_align(state: State<'_, AppState>) -> Result<(), String> {
    state.sca_clear_align().await
}

#[tauri::command]
async fn sca_query(state: State<'_, AppState>, req: nbp_core::sca::Request) -> Result<nbp_core::sca::Result, String> {
    state.sca_query(req).await
}

#[tauri::command]
async fn sca_meta(state: State<'_, AppState>) -> Result<Vec<nbp_core::sca::TraceMeta>, String> {
    state.sca_meta().await
}

#[tauri::command]
async fn sca_export(state: State<'_, AppState>) -> Result<Response, String> {
    state.sca_export().await.map(Response::new)
}

#[tauri::command]
async fn sca_save(state: State<'_, AppState>) -> Result<Response, String> {
    state.sca_save().await.map(Response::new)
}

#[tauri::command]
async fn sca_load(state: State<'_, AppState>, bytes: Vec<u8>) -> Result<nbp_core::sca::Info, String> {
    state.sca_load(bytes).await
}

#[tauri::command]
async fn query_events(state: State<'_, AppState>, since: u64, max: usize) -> Result<Vec<nbp_core::trigger::Event>, String> {
    state.query_events(since, max).await
}

#[tauri::command]
async fn clear_events(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_events().await
}

#[tauri::command]
async fn query_lod(state: State<'_, AppState>, t0: f64, t1: f64, bins: usize) -> Result<Response, String> {
    state.query_lod(t0, t1, bins).await.map(Response::new)
}

#[tauri::command]
async fn query_stats(state: State<'_, AppState>, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
    state.query_stats(t0, t1).await
}

#[tauri::command]
async fn query_value(state: State<'_, AppState>, t: f64) -> Result<Option<(f64, f64, u8, f64)>, String> {
    state.query_value(t).await
}

#[tauri::command]
async fn serial_open(state: State<'_, SerialState>, path: String, cfg: crate::serial::PortCfg, on_data: Channel<Vec<u8>>) -> Result<(), String> {
    state.serial_open(path, cfg, bytes_events(on_data)).await
}

#[tauri::command]
async fn serial_write(state: State<'_, SerialState>, data: Vec<u8>) -> Result<(), String> {
    state.serial_write(data).await
}

#[tauri::command]
async fn serial_signals(state: State<'_, SerialState>, rts: bool, dtr: bool) -> Result<(), String> {
    state.serial_signals(rts, dtr).await
}

#[tauri::command]
async fn serial_close(state: State<'_, SerialState>) -> Result<(), String> {
    state.serial_close().await
}

#[tauri::command]
async fn serial_list() -> Result<Vec<crate::serial::PortInfo>, String> {
    crate::serial::serial_list().await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(SerialState::default())
        .invoke_handler(tauri::generate_handler![
            list_usb, discover, connect, disconnect, recordings_dir, list_recordings, start_recording, stop_recording, start_logging, stop_logging, export_preview, start_export, stop_export, set_log_interval, clear_session, session_lod, session_stats, disk_status, open_log, open_recording, export_csv, count_samples, save_bytes, request, set_buffer, set_calibration, set_trigger, set_math, analyze, sca_new, sca_add, sca_info, sca_align, sca_clear_align, sca_query, sca_meta, sca_export, sca_save, sca_load, query_events, clear_events, query_lod, query_stats, query_value, serial_open, serial_write, serial_signals, serial_close, serial_list
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub mod discovery;
pub mod files;
pub mod link;
pub mod serial;

use link::{Conn, Events, Reply, SharedPipeline, Target};
use nbp_core::buffer::SpanStats;
use nbp_core::Calibration;
use tauri::ipc::Response;
use std::time::Duration;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

#[derive(Default)]
struct AppState {
    conn: Mutex<Option<Conn>>,
    /// the connection's pipeline, apart from `conn` so queries don't wait
    /// for a command in flight
    pipe: std::sync::Mutex<Option<SharedPipeline>>,
    /// applied at connect
    buffer_samples: std::sync::Mutex<Option<usize>>,
    cal: std::sync::Mutex<Calibration>,
    trigger: std::sync::Mutex<nbp_core::trigger::TriggerCfg>,
    /// the math expression, applied at connect / open too
    math: std::sync::Mutex<Option<String>>,
    recording: std::sync::Mutex<Option<files::Writer>>,
    logging: std::sync::Mutex<Option<files::Writer>>,
    exporting: std::sync::Mutex<Option<files::ExportWriter>>,
    /// the side-channel campaign (kept across connections)
    sca: std::sync::Arc<std::sync::Mutex<Option<nbp_core::sca::Campaign>>>,
    /// session log interval, s (applied at connect too)
    log_interval: std::sync::Mutex<Option<f64>>,
}

fn pipe(state: &State<'_, AppState>) -> Option<SharedPipeline> {
    state.pipe.lock().unwrap().clone()
}

#[tauri::command]
async fn list_usb() -> Result<Vec<link::usb::UsbDev>, String> {
    tokio::task::spawn_blocking(link::usb::list).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn discover(timeout_ms: Option<u64>) -> Result<Vec<discovery::NetDev>, String> {
    discovery::discover(Duration::from_millis(timeout_ms.unwrap_or(600))).await
}

/// Opens `target`; Meta and the close event come on `events`.
#[tauri::command]
async fn connect(state: State<'_, AppState>, target: Target, events: Events) -> Result<String, String> {
    let mut slot = state.conn.lock().await;
    slot.take(); // drop (and stop) a previous connection first
    state.pipe.lock().unwrap().take();
    let samples = state.buffer_samples.lock().unwrap().unwrap_or(nbp_core::pipeline::DEFAULT_BUFFER_SAMPLES);
    let cal = state.cal.lock().unwrap().clone();
    let c = Conn::open(target, events, samples, cal).await?;
    c.pipe.lock().unwrap().set_trigger(state.trigger.lock().unwrap().clone());
    if let Some(iv) = *state.log_interval.lock().unwrap() {
        c.pipe.lock().unwrap().set_log_interval(iv);
    }
    let _ = c.pipe.lock().unwrap().set_math(state.math.lock().unwrap().as_deref());
    let d = c.description.clone();
    *state.pipe.lock().unwrap() = Some(c.pipe.clone());
    *slot = Some(c);
    Ok(d)
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let _ = stop_recording(state.clone()).await;
    let _ = stop_logging(state.clone()).await;
    let _ = stop_export(state.clone()).await;
    state.conn.lock().await.take();
    state.pipe.lock().unwrap().take();
    Ok(())
}

// ---- files --------------------------------------------------------------------

#[tauri::command]
fn recordings_dir(app: AppHandle) -> Result<String, String> {
    files::recordings_dir(&app).map(|d| d.display().to_string())
}

#[tauri::command]
fn list_recordings(app: AppHandle) -> Result<Vec<files::RecFile>, String> {
    Ok(files::list(&files::recordings_dir(&app)?))
}

/// Records every sample from now on into the recordings folder; returns the file's path.
#[tauri::command]
async fn start_recording(app: AppHandle, state: State<'_, AppState>, header: String) -> Result<String, String> {
    if state.recording.lock().unwrap().is_some() {
        return Err("already recording".into());
    }
    let conn = state.conn.lock().await;
    let c = conn.as_ref().ok_or("connect to a device to record")?;
    let path = files::recordings_dir(&app)?.join(format!("{}.{}", files::timestamp(), files::EXT));
    let rec = files::start(path.clone(), &header, &c.pipe, &c.rec)?;
    *state.recording.lock().unwrap() = Some(rec);
    Ok(path.display().to_string())
}

#[tauri::command]
async fn stop_recording(state: State<'_, AppState>) -> Result<Option<files::RecordingDone>, String> {
    let Some(rec) = state.recording.lock().unwrap().take() else { return Ok(None) };
    let conn = state.conn.lock().await;
    let c = conn.as_ref().ok_or("connection gone")?;
    let (pipe, sink) = (c.pipe.clone(), c.rec.clone());
    drop(conn);
    tokio::task::spawn_blocking(move || files::stop(rec, &pipe, &sink)).await.map_err(|e| e.to_string())?.map(Some)
}

// ---- logging (the session log to a file) -------------------------------------------

/// Starts writing the session log to ~/Documents/Nebulosa/<time>.nblog; returns the path.
#[tauri::command]
async fn start_logging(app: AppHandle, state: State<'_, AppState>, header: String) -> Result<String, String> {
    if state.logging.lock().unwrap().is_some() {
        return Err("already logging".into());
    }
    let conn = state.conn.lock().await;
    let c = conn.as_ref().ok_or("connect to a device to log")?;
    let path = files::recordings_dir(&app)?.join(format!("{}.{}", files::timestamp(), files::LOG_EXT));
    let w = files::start_log(path.clone(), &header, &c.pipe, &c.log)?;
    *state.logging.lock().unwrap() = Some(w);
    Ok(path.display().to_string())
}

#[tauri::command]
async fn stop_logging(state: State<'_, AppState>) -> Result<Option<files::LoggingDone>, String> {
    let Some(w) = state.logging.lock().unwrap().take() else { return Ok(None) };
    let conn = state.conn.lock().await;
    let c = conn.as_ref().ok_or("connection gone")?;
    let (pipe, sink) = (c.pipe.clone(), c.log.clone());
    drop(conn);
    tokio::task::spawn_blocking(move || files::stop_log(w, &pipe, &sink)).await.map_err(|e| e.to_string())?.map(Some)
}

#[tauri::command]
async fn export_preview(
    state: State<'_, AppState>,
    cfg: nbp_core::export::ExportCfg,
    wall_offset: f64,
    rows: usize,
) -> Result<String, String> {
    Ok(pipe(&state).map(|p| p.lock().unwrap().export_preview(&cfg, wall_offset, rows.min(50))).unwrap_or_default())
}

/// Starts an export in the logger's chosen format into ~/Documents/Nebulosa; returns the path.
#[tauri::command]
async fn start_export(
    app: AppHandle,
    state: State<'_, AppState>,
    cfg: nbp_core::export::ExportCfg,
    wall_offset: f64,
    header: String,
) -> Result<String, String> {
    if state.exporting.lock().unwrap().is_some() {
        return Err("already exporting".into());
    }
    if cfg.fields.is_empty() {
        return Err("no fields chosen".into());
    }
    let conn = state.conn.lock().await;
    let c = conn.as_ref().ok_or("connect to a device to log")?;
    let path = files::recordings_dir(&app)?.join(format!("{}.{}", files::timestamp(), files::export_ext(&cfg.format)));
    let w = files::start_export(path.clone(), cfg, wall_offset, &header, &c.pipe, &c.export)?;
    *state.exporting.lock().unwrap() = Some(w);
    Ok(path.display().to_string())
}

#[tauri::command]
async fn stop_export(state: State<'_, AppState>) -> Result<Option<files::ExportDone>, String> {
    let Some(w) = state.exporting.lock().unwrap().take() else { return Ok(None) };
    let conn = state.conn.lock().await;
    let c = conn.as_ref().ok_or("connection gone")?;
    let (pipe, sink) = (c.pipe.clone(), c.export.clone());
    drop(conn);
    tokio::task::spawn_blocking(move || files::stop_export(w, &pipe, &sink)).await.map_err(|e| e.to_string())?.map(Some)
}

/// The session log's interval, s; it starts over.
#[tauri::command]
async fn set_log_interval(state: State<'_, AppState>, interval: f64) -> Result<(), String> {
    let iv = interval.clamp(1e-3, 3600.0);
    *state.log_interval.lock().unwrap() = Some(iv);
    if let Some(p) = pipe(&state) {
        p.lock().unwrap().set_log_interval(iv);
    }
    Ok(())
}

#[tauri::command]
async fn clear_session(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(p) = pipe(&state) {
        p.lock().unwrap().clear_session();
    }
    Ok(())
}

#[tauri::command]
async fn session_lod(state: State<'_, AppState>, t0: f64, t1: f64, bins: usize) -> Result<Response, String> {
    let v = match pipe(&state) {
        Some(p) => p.lock().unwrap().session_lod(t0, t1, bins.min(8192)),
        None => vec![3.0, 0.0, 18.0, 0.0],
    };
    Ok(Response::new(v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()))
}

#[tauri::command]
async fn session_stats(state: State<'_, AppState>, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
    Ok(pipe(&state).map(|p| p.lock().unwrap().session_stats(t0, t1)))
}

#[derive(serde::Serialize)]
struct DiskStatus {
    /// bytes free where the files go
    free: u64,
    recording_error: Option<String>,
    logging_error: Option<String>,
    export_error: Option<String>,
    /// the export file's size so far
    export_bytes: u64,
    dropped: u64,
}

/// For the UI's watchdog: free space, and whether a writer failed or fell behind.
#[tauri::command]
fn disk_status(app: AppHandle, state: State<'_, AppState>) -> Result<DiskStatus, String> {
    let dir = files::recordings_dir(&app)?;
    let err = |w: &std::sync::Mutex<Option<files::Writer>>| w.lock().unwrap().as_ref().and_then(|w| w.error.lock().unwrap().clone());
    let dropped = [&state.recording, &state.logging]
        .iter()
        .filter_map(|w| w.lock().unwrap().as_ref().map(|w| w.dropped.load(std::sync::atomic::Ordering::Relaxed)))
        .sum::<u64>()
        + state.exporting.lock().unwrap().as_ref().map_or(0, |w| w.dropped.load(std::sync::atomic::Ordering::Relaxed));
    let export_error = state.exporting.lock().unwrap().as_ref().and_then(|w| w.error.lock().unwrap().clone());
    let export_bytes = state.exporting.lock().unwrap().as_ref().and_then(|w| std::fs::metadata(&w.path).ok()).map_or(0, |m| m.len());
    Ok(DiskStatus {
        free: files::free_space(&dir),
        recording_error: err(&state.recording),
        logging_error: err(&state.logging),
        export_error,
        export_bytes,
        dropped,
    })
}

/// Opens a session log file (.nblog) for viewing; returns its header (JSON).
#[tauri::command]
async fn open_log(state: State<'_, AppState>, path: String, events: Events) -> Result<String, String> {
    let _ = stop_recording(state.clone()).await;
    let _ = stop_logging(state.clone()).await;
    state.conn.lock().await.take();
    let (p, header) = tokio::task::spawn_blocking(move || files::open_log(&path, &events)).await.map_err(|e| e.to_string())??;
    *state.pipe.lock().unwrap() = Some(std::sync::Arc::new(std::sync::Mutex::new(p)));
    Ok(header)
}

/// Opens a recording for playback (instead of a device); returns its header (JSON).
#[tauri::command]
async fn open_recording(state: State<'_, AppState>, path: String, events: Events) -> Result<String, String> {
    let _ = stop_recording(state.clone()).await;
    state.conn.lock().await.take();
    let math = state.math.lock().unwrap().clone();
    let (mut p, header) = tokio::task::spawn_blocking(move || files::open(&path, &events)).await.map_err(|e| e.to_string())??;
    let _ = p.set_math(math.as_deref());
    *state.pipe.lock().unwrap() = Some(std::sync::Arc::new(std::sync::Mutex::new(p)));
    Ok(header)
}

/// Every sample in [t0, t1) to a CSV file; returns the rows written.
#[tauri::command]
async fn export_csv(state: State<'_, AppState>, t0: f64, t1: f64, path: String) -> Result<u64, String> {
    let p = pipe(&state).ok_or("no data")?;
    tokio::task::spawn_blocking(move || files::export_csv(&p, t0, t1, &path)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn count_samples(state: State<'_, AppState>, t0: f64, t1: f64) -> Result<u64, String> {
    Ok(pipe(&state).map(|p| p.lock().unwrap().count_samples(t0, t1)).unwrap_or(0))
}

/// Writes bytes (a PNG) to a file the user picked.
#[tauri::command]
async fn save_bytes(path: String, bytes: Vec<u8>) -> Result<(), String> {
    std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))
}

#[tauri::command]
async fn request(
    state: State<'_, AppState>,
    cmd: u8,
    payload: Vec<u8>,
    timeout_ms: Option<u64>,
) -> Result<Reply, String> {
    let mut slot = state.conn.lock().await;
    let c = slot.as_mut().ok_or("not connected")?;
    c.request(cmd, &payload, Duration::from_millis(timeout_ms.unwrap_or(1500))).await
}

/// Samples the buffer holds; resizing drops what it holds.
#[tauri::command]
async fn set_buffer(state: State<'_, AppState>, samples: usize) -> Result<(), String> {
    *state.buffer_samples.lock().unwrap() = Some(samples);
    if let Some(p) = pipe(&state) {
        p.lock().unwrap().set_buffer_samples(samples);
    }
    Ok(())
}

#[tauri::command]
async fn set_calibration(state: State<'_, AppState>, cal: Calibration) -> Result<(), String> {
    *state.cal.lock().unwrap() = cal.clone();
    if let Some(p) = pipe(&state) {
        p.lock().unwrap().set_calibration(cal);
    }
    Ok(())
}

#[tauri::command]
async fn set_trigger(state: State<'_, AppState>, cfg: nbp_core::trigger::TriggerCfg) -> Result<(), String> {
    *state.trigger.lock().unwrap() = cfg.clone();
    if let Some(p) = pipe(&state) {
        p.lock().unwrap().set_trigger(cfg);
    }
    Ok(())
}

/// The math channel: an expression (see nbp-core math.rs) or none. Errors say what doesn't parse.
#[tauri::command]
async fn set_math(state: State<'_, AppState>, expr: Option<String>) -> Result<(), String> {
    if let Some(e) = &expr {
        nbp_core::math::MathProgram::compile(e)?;
    }
    *state.math.lock().unwrap() = expr.clone();
    if let Some(p) = pipe(&state) {
        // computing the buffer again can take a moment: off the async threads
        tokio::task::spawn_blocking(move || p.lock().unwrap().set_math(expr.as_deref())).await.map_err(|e| e.to_string())??;
    }
    Ok(())
}

/// Analysis over [t0, t1): the samples are copied out under the lock, analysed outside it.
#[tauri::command]
async fn analyze(
    state: State<'_, AppState>,
    t0: f64,
    t1: f64,
    req: nbp_core::analysis::Request,
) -> Result<nbp_core::analysis::Result, String> {
    let p = pipe(&state).ok_or("no data")?;
    tokio::task::spawn_blocking(move || {
        let x = p.lock().unwrap().extract(t0, t1);
        nbp_core::analysis::run(&x, &req)
    })
    .await
    .map_err(|e| e.to_string())
}

// ---- side-channel campaign -------------------------------------------------------

#[tauri::command]
async fn sca_new(state: State<'_, AppState>, cfg: nbp_core::sca::Config) -> Result<(), String> {
    *state.sca.lock().unwrap() = Some(nbp_core::sca::Campaign::new(cfg)?);
    Ok(())
}

/// [t0, t1) of the buffer as one more trace: the traces so far.
#[tauri::command]
async fn sca_add(state: State<'_, AppState>, t0: f64, t1: f64, meta: nbp_core::sca::TraceMeta) -> Result<usize, String> {
    let x = pipe(&state).ok_or("no data")?.lock().unwrap().extract(t0, t1);
    state.sca.lock().unwrap().as_mut().ok_or("no campaign")?.add(&x, meta)
}

#[tauri::command]
async fn sca_info(state: State<'_, AppState>) -> Result<Option<nbp_core::sca::Info>, String> {
    Ok(state.sca.lock().unwrap().as_ref().map(|c| c.info()))
}

#[tauri::command]
async fn sca_align(state: State<'_, AppState>, a: usize, b: usize, max_shift: usize) -> Result<nbp_core::sca::AlignResult, String> {
    let sca = state.sca.clone();
    tokio::task::spawn_blocking(move || sca.lock().unwrap().as_mut().ok_or("no campaign".to_string())?.align(a, b, max_shift))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sca_clear_align(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(c) = state.sca.lock().unwrap().as_mut() {
        c.clear_align();
    }
    Ok(())
}

#[tauri::command]
async fn sca_query(state: State<'_, AppState>, req: nbp_core::sca::Request) -> Result<nbp_core::sca::Result, String> {
    let sca = state.sca.clone();
    tokio::task::spawn_blocking(move || sca.lock().unwrap().as_ref().map(|c| c.query(&req)).ok_or("no campaign".to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sca_meta(state: State<'_, AppState>) -> Result<Vec<nbp_core::sca::TraceMeta>, String> {
    Ok(state.sca.lock().unwrap().as_ref().map(|c| c.meta().to_vec()).unwrap_or_default())
}

/// The campaign as an .npz, raw bytes.
#[tauri::command]
async fn sca_export(state: State<'_, AppState>) -> Result<Response, String> {
    let sca = state.sca.clone();
    tokio::task::spawn_blocking(move || sca.lock().unwrap().as_ref().map(|c| Response::new(c.export_npz())).ok_or("no campaign".to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Trigger events with seq ≥ since, at most max.
#[tauri::command]
async fn query_events(state: State<'_, AppState>, since: u64, max: usize) -> Result<Vec<nbp_core::trigger::Event>, String> {
    Ok(pipe(&state).map(|p| p.lock().unwrap().events_since(since, max.min(10_000))).unwrap_or_default())
}

#[tauri::command]
async fn clear_events(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(p) = pipe(&state) {
        p.lock().unwrap().clear_events();
    }
    Ok(())
}

/// [t0, t1] in `bins` columns: SampleBuffer::lod's flat f64 layout, raw bytes.
#[tauri::command]
async fn query_lod(state: State<'_, AppState>, t0: f64, t1: f64, bins: usize) -> Result<Response, String> {
    let v = match pipe(&state) {
        Some(p) => p.lock().unwrap().lod(t0, t1, bins.min(8192)),
        None => vec![2.0, 0.0, 14.0, 0.0],
    };
    Ok(Response::new(v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()))
}

#[tauri::command]
async fn query_stats(state: State<'_, AppState>, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
    Ok(pipe(&state).map(|p| p.lock().unwrap().stats(t0, t1)))
}

#[tauri::command]
async fn query_value(state: State<'_, AppState>, t: f64) -> Result<Option<(f64, f64, u8, f64)>, String> {
    Ok(pipe(&state).and_then(|p| p.lock().unwrap().value_at(t)))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(serial::SerialState::default())
        .invoke_handler(tauri::generate_handler![
            list_usb, discover, connect, disconnect, request, set_buffer, set_calibration, set_trigger, query_lod, query_stats,
            query_value, recordings_dir, list_recordings, start_recording, stop_recording, open_recording, export_csv,
            count_samples, save_bytes, query_events, clear_events, analyze, set_math, start_logging, stop_logging, set_log_interval, clear_session, session_lod, session_stats,
            disk_status, open_log, start_export, stop_export, export_preview, sca_new, sca_add, sca_info, sca_align,
            sca_clear_align, sca_query, sca_export, sca_meta, serial::serial_list, serial::serial_open, serial::serial_write, serial::serial_signals, serial::serial_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

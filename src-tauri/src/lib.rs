mod discovery;
mod files;
mod link;

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
    recording: std::sync::Mutex<Option<files::ActiveRecording>>,
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
    let d = c.description.clone();
    *state.pipe.lock().unwrap() = Some(c.pipe.clone());
    *slot = Some(c);
    Ok(d)
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let _ = stop_recording(state.clone()).await;
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

/// Opens a recording for playback (instead of a device); returns its header (JSON).
#[tauri::command]
async fn open_recording(state: State<'_, AppState>, path: String, events: Events) -> Result<String, String> {
    let _ = stop_recording(state.clone()).await;
    state.conn.lock().await.take();
    let (p, header) = tokio::task::spawn_blocking(move || files::open(&path, &events)).await.map_err(|e| e.to_string())??;
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
async fn query_value(state: State<'_, AppState>, t: f64) -> Result<Option<(f64, f64, u8)>, String> {
    Ok(pipe(&state).and_then(|p| p.lock().unwrap().value_at(t)))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_usb, discover, connect, disconnect, request, set_buffer, set_calibration, set_trigger, query_lod, query_stats,
            query_value, recordings_dir, list_recordings, start_recording, stop_recording, open_recording, export_csv,
            count_samples, save_bytes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

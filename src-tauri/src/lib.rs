mod discovery;
mod link;

use link::{Conn, Events, Reply, Target};
use nbp_core::Calibration;
use std::time::Duration;
use tauri::State;
use tokio::sync::Mutex;

#[derive(Default)]
struct AppState {
    conn: Mutex<Option<Conn>>,
}

#[tauri::command]
async fn list_usb() -> Result<Vec<link::usb::UsbDev>, String> {
    tokio::task::spawn_blocking(link::usb::list).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn discover(timeout_ms: Option<u64>) -> Result<Vec<discovery::NetDev>, String> {
    discovery::discover(Duration::from_millis(timeout_ms.unwrap_or(600))).await
}

/// Opens `target`; batches and the close event come on `events`.
#[tauri::command]
async fn connect(state: State<'_, AppState>, target: Target, events: Events) -> Result<String, String> {
    let mut slot = state.conn.lock().await;
    slot.take(); // drop (and stop) a previous connection first
    let c = Conn::open(target, events).await?;
    let d = c.description.clone();
    *slot = Some(c);
    Ok(d)
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    state.conn.lock().await.take();
    Ok(())
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

#[tauri::command]
async fn set_bucket(state: State<'_, AppState>, n: u32) -> Result<(), String> {
    if let Some(c) = state.conn.lock().await.as_ref() {
        c.set_bucket(n);
    }
    Ok(())
}

#[tauri::command]
async fn set_calibration(state: State<'_, AppState>, cal: Calibration) -> Result<(), String> {
    if let Some(c) = state.conn.lock().await.as_ref() {
        c.set_calibration(cal);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![list_usb, discover, connect, disconnect, request, set_bucket, set_calibration])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

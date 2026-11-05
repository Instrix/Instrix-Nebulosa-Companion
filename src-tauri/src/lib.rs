pub mod api;
pub mod discovery;
pub mod events;
pub mod files;
pub mod link;
pub mod serial;

#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "tauri-app")]
mod shell_tauri;

#[cfg(feature = "tauri-app")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    shell_tauri::run()
}

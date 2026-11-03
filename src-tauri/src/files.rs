//! Files: recording to disk (a writer thread fed by the dispatcher), the
//! recordings folder, opening a recording for playback, CSV export, saving
//! bytes (PNG).

use crate::link::{Events, RecSink, SharedPipeline};
use nbp_core::recording::RecordingStatus;
use nbp_core::Pipeline;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use tauri::{AppHandle, Manager};

pub const EXT: &str = "nbrec";

/// ~/Documents/Nebulosa (created on demand)
pub fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().document_dir().or_else(|_| app.path().home_dir()).map_err(|e| e.to_string())?;
    let d = base.join("Nebulosa");
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d)
}

/// UTC "YYYY-MM-DD_HH-MM-SS" (no time zone database needed)
pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub struct ActiveRecording {
    pub path: PathBuf,
    thread: JoinHandle<Result<(), String>>,
}

#[derive(Serialize)]
pub struct RecordingDone {
    pub path: String,
    #[serde(flatten)]
    pub status: RecordingStatus,
}

/// Opens the file, starts the writer thread, and turns recording on in the
/// pipeline; the dispatcher feeds the thread from then on.
pub fn start(path: PathBuf, header_json: &str, pipe: &SharedPipeline, sink: &RecSink) -> Result<ActiveRecording, String> {
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let thread = std::thread::spawn(move || -> Result<(), String> {
        let mut w = std::io::BufWriter::with_capacity(8 << 20, file);
        for chunk in rx {
            w.write_all(&chunk).map_err(|e| e.to_string())?;
        }
        w.flush().map_err(|e| e.to_string())?;
        w.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())
    });
    // lock order as in the dispatcher: pipeline, then sink
    let mut p = pipe.lock().unwrap();
    p.start_recording(header_json);
    *sink.lock().unwrap() = Some(tx);
    Ok(ActiveRecording { path, thread })
}

/// Stops recording: the last bytes, then waits for the writer to finish.
pub fn stop(rec: ActiveRecording, pipe: &SharedPipeline, sink: &RecSink) -> Result<RecordingDone, String> {
    let (tx, last) = {
        let mut p = pipe.lock().unwrap();
        let tx = sink.lock().unwrap().take();
        (tx, p.stop_recording())
    };
    let mut status = RecordingStatus::default();
    if let (Some(tx), Some((bytes, st))) = (tx, last) {
        let _ = tx.send(bytes);
        status = st;
    }
    // the sender is dropped here: the thread drains, flushes and ends
    rec.thread.join().map_err(|_| "recording writer crashed".to_string())??;
    Ok(RecordingDone { path: rec.path.display().to_string(), status })
}

#[derive(Serialize)]
pub struct RecFile {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    /// ms since 1970
    pub modified: u64,
}

pub fn list(dir: &Path) -> Vec<RecFile> {
    let mut v: Vec<RecFile> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == EXT))
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            Some(RecFile {
                path: e.path().display().to_string(),
                name: e.file_name().to_string_lossy().into_owned(),
                bytes: md.len(),
                modified: md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            })
        })
        .collect();
    v.sort_by(|a, b| b.modified.cmp(&a.modified));
    v
}

/// A recording, loaded for playback; its Meta goes out on `events` once.
pub fn open(path: &str, events: &Events) -> Result<(Pipeline, String), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let p = Pipeline::from_recording(&bytes)?;
    let header = p.file_header().unwrap_or("{}").to_string();
    crate::link::send_meta(events, p.meta());
    Ok((p, header))
}

/// Every sample in [t0, t1) as CSV; returns the rows written.
pub fn export_csv(pipe: &SharedPipeline, t0: f64, t1: f64, path: &str) -> Result<u64, String> {
    let f = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let mut w = std::io::BufWriter::with_capacity(4 << 20, f);
    let p = pipe.lock().unwrap();
    let math = p.has_math();
    writeln!(w, "time_s,current_A,voltage_V,power_W,range{}", if math { ",math" } else { "" }).map_err(|e| e.to_string())?;
    let mut rows = 0u64;
    let mut err = None;
    p.for_each_sample(t0, t1, |t, i, v, r, m| {
        if err.is_none() {
            let res = if math {
                writeln!(w, "{t:.9},{i:.6e},{v:.6e},{:.6e},{r},{m:.6e}", i * v)
            } else {
                writeln!(w, "{t:.9},{i:.6e},{v:.6e},{:.6e},{r}", i * v)
            };
            if let Err(e) = res {
                err = Some(e.to_string());
            }
            rows += 1;
        }
    });
    drop(p);
    if let Some(e) = err {
        return Err(e);
    }
    w.flush().map_err(|e| e.to_string())?;
    Ok(rows)
}

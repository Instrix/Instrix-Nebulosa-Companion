//! Files: recording to disk (a writer thread fed by the dispatcher), the
//! recordings folder, opening a recording for playback, CSV export, saving
//! bytes (PNG).

use crate::link::{Events, RecSink, SharedPipeline, Sink};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use nbp_core::recording::RecordingStatus;
use nbp_core::Pipeline;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use tauri::{AppHandle, Manager};

pub const EXT: &str = "nbrec";
pub const LOG_EXT: &str = "nblog";

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

/// A file being written by its own thread, fed through a bounded queue.
pub struct Writer {
    pub path: PathBuf,
    thread: JoinHandle<Result<(), String>>,
    /// the first write error (a full disk): the UI shows it and stops
    pub error: Arc<Mutex<Option<String>>>,
    pub dropped: Arc<AtomicU64>,
}

/// Queue length in chunks (a chunk ≤ 64 Ki samples = 256 KB): ≤ ~64 MB waiting.
const QUEUE: usize = 256;

fn spawn_writer(path: PathBuf) -> Result<(Writer, Sink), String> {
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE);
    let error: Arc<Mutex<Option<String>>> = Arc::default();
    let err2 = error.clone();
    let thread = std::thread::spawn(move || -> Result<(), String> {
        let mut w = std::io::BufWriter::with_capacity(8 << 20, file);
        let mut failed = false;
        for chunk in rx {
            if failed {
                continue; // drain, so the sender never blocks
            }
            if let Err(e) = w.write_all(&chunk) {
                *err2.lock().unwrap() = Some(e.to_string());
                failed = true;
            }
        }
        if failed {
            return Err(err2.lock().unwrap().clone().unwrap_or_default());
        }
        w.flush().map_err(|e| e.to_string())?;
        w.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())
    });
    let dropped: Arc<AtomicU64> = Arc::default();
    Ok((Writer { path, thread, error, dropped: dropped.clone() }, Sink { tx, dropped }))
}

impl Writer {
    fn finish(self) -> Result<PathBuf, String> {
        self.thread.join().map_err(|_| "file writer crashed".to_string())??;
        let lost = self.dropped.load(Ordering::Relaxed);
        if lost > 0 {
            return Err(format!("{}: the disk didn't keep up, {lost} chunks were lost", self.path.display()));
        }
        Ok(self.path)
    }
}

#[derive(Serialize)]
pub struct RecordingDone {
    pub path: String,
    #[serde(flatten)]
    pub status: RecordingStatus,
}

/// Opens the file, starts the writer thread, and turns recording on in the
/// pipeline; the dispatcher feeds the thread from then on.
pub fn start(path: PathBuf, header_json: &str, pipe: &SharedPipeline, sink: &RecSink) -> Result<Writer, String> {
    let (w, s) = spawn_writer(path)?;
    // lock order as in the dispatcher: pipeline, then sink
    let mut p = pipe.lock().unwrap();
    p.start_recording(header_json);
    *sink.lock().unwrap() = Some(s);
    Ok(w)
}

/// Stops recording: the last bytes, then waits for the writer to finish.
pub fn stop(rec: Writer, pipe: &SharedPipeline, sink: &RecSink) -> Result<RecordingDone, String> {
    let (s, last) = {
        let mut p = pipe.lock().unwrap();
        let s = sink.lock().unwrap().take();
        (s, p.stop_recording())
    };
    let mut status = RecordingStatus::default();
    if let (Some(s), Some((bytes, st))) = (s, last) {
        s.send(bytes);
        status = st;
    }
    // the sender is dropped here: the thread drains, flushes and ends
    let path = rec.finish()?;
    Ok(RecordingDone { path: path.display().to_string(), status })
}

// ---- the session log file -------------------------------------------------------

#[derive(Serialize)]
pub struct LoggingDone {
    pub path: String,
    pub entries: u64,
    pub bytes: u64,
}

pub fn start_log(path: PathBuf, header_json: &str, pipe: &SharedPipeline, sink: &RecSink) -> Result<Writer, String> {
    let (w, s) = spawn_writer(path)?;
    let mut p = pipe.lock().unwrap();
    p.start_logging(header_json);
    *sink.lock().unwrap() = Some(s);
    Ok(w)
}

pub fn stop_log(w: Writer, pipe: &SharedPipeline, sink: &RecSink) -> Result<LoggingDone, String> {
    let (s, last) = {
        let mut p = pipe.lock().unwrap();
        let s = sink.lock().unwrap().take();
        // what was taken but not yet sent goes first
        let pending = p.take_log();
        (s, (pending, p.stop_logging()))
    };
    let mut entries = 0;
    if let Some(s) = s {
        if !last.0.is_empty() {
            s.send(last.0);
        }
        if let Some((bytes, st)) = last.1 {
            s.send(bytes);
            entries = st.entries;
        }
    }
    let path = w.finish()?;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(LoggingDone { path: path.display().to_string(), entries, bytes })
}

/// A log file opened for viewing; its Meta goes out on `events` once.
pub fn open_log(path: &str, events: &Events) -> Result<(Pipeline, String), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let p = Pipeline::from_log(&bytes)?;
    let header = p.file_header().unwrap_or("{}").to_string();
    crate::link::send_meta(events, p.meta());
    Ok((p, header))
}

/// Free space where the recordings go, bytes.
pub fn free_space(dir: &Path) -> u64 {
    fs2::available_space(dir).unwrap_or(u64::MAX)
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
        .filter(|e| e.path().extension().is_some_and(|x| x == EXT || x == LOG_EXT))
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

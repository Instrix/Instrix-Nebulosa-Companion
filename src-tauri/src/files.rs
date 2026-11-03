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

// ---- exports (the logger's formats) ------------------------------------------------

use crate::link::{ChunkSink, ChunkSinkInner};
use nbp_core::export::{Chunk, ExportCfg, Exporter, Format};

/// Chunks waiting for the formatter: 64 of up to 64 Ki samples (~2 s at 2 MS/s).
const EXPORT_QUEUE: usize = 64;
/// HDF5 chunk length (elements per dataset chunk). Not 65536: hdf5-pure 0.47
/// encodes that chunk dimension in a width libhdf5 2.0 rejects ("stored chunk
/// dimension encoding length does not match"); 65535 and below read fine.
const H5_CHUNK: usize = 32768;

pub struct ExportWriter {
    pub path: PathBuf,
    thread: JoinHandle<Result<u64, String>>,
    pub error: Arc<Mutex<Option<String>>>,
    pub dropped: Arc<AtomicU64>,
}

#[derive(Serialize)]
pub struct ExportDone {
    pub path: String,
    pub rows: u64,
    pub bytes: u64,
}

pub fn export_ext(f: &Format) -> &'static str {
    match f {
        Format::Csv { .. } => "csv",
        Format::Txt { .. } => "txt",
        Format::Jsonl => "jsonl",
        Format::Hdf5 => "h5",
    }
}

/// Starts exporting: the formatter thread, the metadata sidecar (<file>.json),
/// and the pipeline's tap; the dispatcher feeds the thread from then on.
pub fn start_export(
    path: PathBuf,
    cfg: ExportCfg,
    wall_offset: f64,
    header_json: &str,
    pipe: &SharedPipeline,
    sink: &ChunkSink,
) -> Result<ExportWriter, String> {
    let cal = pipe.lock().unwrap().calibration().clone();
    let meta = format!("{{\"export\":{},\"session\":{}}}", serde_json::to_string(&cfg).unwrap_or_default(), header_json);
    let side = path.with_extension(format!("{}.json", export_ext(&cfg.format)));
    std::fs::write(&side, meta).map_err(|e| format!("{}: {e}", side.display()))?;
    let (tx, rx) = mpsc::sync_channel::<Chunk>(EXPORT_QUEUE);
    let error: Arc<Mutex<Option<String>>> = Arc::default();
    let err2 = error.clone();
    let p2 = path.clone();
    let header = header_json.to_string();
    let data = cfg.data;
    let thread = std::thread::spawn(move || -> Result<u64, String> {
        let mut x = Exporter::new(cfg.clone(), cal, wall_offset);
        let res = if matches!(cfg.format, Format::Hdf5) { write_h5(&p2, &mut x, &header, rx) } else { write_text(&p2, &mut x, rx) };
        if let Err(e) = &res {
            *err2.lock().unwrap() = Some(e.clone());
        }
        res.map(|_| x.rows)
    });
    let dropped: Arc<AtomicU64> = Arc::default();
    let mut p = pipe.lock().unwrap();
    p.start_export(data);
    *sink.lock().unwrap() = Some(ChunkSinkInner { tx, dropped: dropped.clone() });
    Ok(ExportWriter { path, thread, error, dropped })
}

fn write_text(path: &Path, x: &mut Exporter, rx: mpsc::Receiver<Chunk>) -> Result<(), String> {
    let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut w = std::io::BufWriter::with_capacity(8 << 20, f);
    w.write_all(&x.header()).map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity(4 << 20);
    for c in rx {
        buf.clear();
        x.text(&c, &mut buf);
        w.write_all(&buf).map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())?;
    w.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())
}

/// HDF5: one f64 dataset per field (unlimited, chunked), appended a chunk at a time.
fn write_h5(path: &Path, x: &mut Exporter, header: &str, rx: mpsc::Receiver<Chunk>) -> Result<(), String> {
    use hdf5_pure::{AttrValue, File, FileAccessProperties, FileBuilder, MaxExtent, SyncPolicy};
    let e = |e: hdf5_pure::Error| e.to_string();
    // dataset names from the labels: unique, no '/'
    let mut names: Vec<String> = Vec::new();
    for f in &x.cfg().fields {
        let mut n: String = f.label.chars().map(|c| if c == '/' || c == '.' { '_' } else { c }).collect();
        if n.trim().is_empty() {
            n = format!("{:?}", f.what).to_lowercase();
        }
        let base = n.clone();
        let mut k = 2;
        while names.contains(&n) {
            n = format!("{base}_{k}");
            k += 1;
        }
        names.push(n);
    }
    let mut b = FileBuilder::new();
    b.set_attr("nebulosa", AttrValue::String(header.to_string()));
    for (n, f) in names.iter().zip(&x.cfg().fields) {
        b.create_dataset(n)
            .with_f64_data(&[])
            .with_shape(&[0])
            .with_maxshape(&[MaxExtent::Unlimited])
            .with_chunks(&[H5_CHUNK as u64])
            .set_attr("label", AttrValue::String(f.label.clone()))
            .set_attr("scale", AttrValue::F64(f.scale));
    }
    b.write(path).map_err(e)?;
    // no fsync per append (each append is still crash-atomic); one at close
    let file = File::open_rw_with_options(path, FileAccessProperties::new().with_sync_policy(SyncPolicy::OnClose)).map_err(e)?;
    let mut cols: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    let flush = |cols: &mut Vec<Vec<f64>>, all: bool| -> Result<(), String> {
        for (k, n) in names.iter().enumerate() {
            let take = if all { cols[k].len() } else { cols[k].len() / H5_CHUNK * H5_CHUNK };
            if take > 0 {
                file.dataset(n).map_err(e)?.append(&cols[k][..take]).map_err(e)?;
                cols[k].drain(..take);
            }
        }
        Ok(())
    };
    for c in rx {
        for (k, v) in x.columns(&c).into_iter().enumerate() {
            cols[k].extend(v);
        }
        flush(&mut cols, false)?;
    }
    flush(&mut cols, true)?;
    file.close().map_err(e)
}

/// Stops the export: the last chunks, then waits for the writer.
pub fn stop_export(w: ExportWriter, pipe: &SharedPipeline, sink: &ChunkSink) -> Result<ExportDone, String> {
    let (s, last) = {
        let mut p = pipe.lock().unwrap();
        let s = sink.lock().unwrap().take();
        (s, p.stop_export())
    };
    if let Some(s) = s {
        for c in last {
            // the last ones may wait: the stream is no longer feeding the queue
            if s.tx.send(c).is_err() {
                break;
            }
        }
    }
    let rows = w.thread.join().map_err(|_| "export writer crashed".to_string())??;
    let lost = w.dropped.load(Ordering::Relaxed);
    if lost > 0 {
        return Err(format!("{}: the writer didn't keep up, {lost} chunks were lost", w.path.display()));
    }
    let bytes = std::fs::metadata(&w.path).map(|m| m.len()).unwrap_or(0);
    Ok(ExportDone { path: w.path.display().to_string(), rows, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nbp_core::buffer::pack;
    use nbp_core::export::*;

    fn cfg(format: Format) -> ExportCfg {
        ExportCfg {
            data: Data::Raw,
            format,
            time: TimeFmt::Relative,
            fields: vec![
                Field { what: What::Time, stat: Stat::Mean, label: "t (s)".into(), scale: 1.0, decimals: 7 },
                Field { what: What::I, stat: Stat::Mean, label: "I (mA)".into(), scale: 1e3, decimals: 4 },
            ],
        }
    }

    fn chunk(k: u64) -> Chunk {
        Chunk::Raw { ts0: k * 65536, step: 1, packed: (0..65536).map(|_| pack(8192, 3300, 0, false)).collect(), math: vec![f32::NAN; 65536] }
    }

    #[test]
    fn csv_writer() {
        let path = std::env::temp_dir().join(format!("neb-csv-{}.csv", std::process::id()));
        let mut c = cfg(Format::Csv { separator: ";".into(), decimal_comma: true, header: true });
        c.fields[0].decimals = 7;
        let mut x = Exporter::new(c, Default::default(), 0.0);
        let (tx, rx) = mpsc::sync_channel(8);
        let p2 = path.clone();
        let h = std::thread::spawn(move || write_text(&p2, &mut x, rx).map(|_| x.rows));
        tx.send(chunk(0)).unwrap();
        drop(tx);
        assert_eq!(h.join().unwrap().unwrap(), 65536);
        let s = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 65537);
        assert_eq!(lines[0], "t (s);I (mA)");
        assert_eq!(lines[2], "0,0000005;1000,0000");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn h5_roundtrip() {
        let dir = std::env::temp_dir().join(format!("neb-h5-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.h5");
        let mut x = Exporter::new(cfg(Format::Hdf5), Default::default(), 0.0);
        let (tx, rx) = mpsc::sync_channel(8);
        let p2 = path.clone();
        let h = std::thread::spawn(move || write_h5(&p2, &mut x, "{\"a\":1}", rx).map(|_| x.rows));
        for k in 0..3 {
            tx.send(chunk(k)).unwrap();
        }
        drop(tx);
        assert_eq!(h.join().unwrap().unwrap(), 3 * 65536);
        let f = hdf5_pure::File::open(&path).unwrap();
        let i = f.dataset("I (mA)").unwrap().read_f64().unwrap();
        let t = f.dataset("t (s)").unwrap().read_f64().unwrap();
        assert_eq!(i.len(), 3 * 65536);
        assert!((i[100] - 1000.0).abs() < 1e-9);
        assert!((t[65536 * 2] - 65536.0 * 2.0 * 500e-9).abs() < 1e-12);
        println!("h5 at {}", path.display());
    }
}

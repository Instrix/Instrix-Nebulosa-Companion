//! What the UI can ask of the backend: one method per command. The desktop
//! shells call these (Tauri commands in shell_tauri.rs, the WebSocket in
//! server.rs); the names and arguments are the same in both.

use crate::events::Events;
use crate::link::{self, Conn, Reply, SharedPipeline, Target};
use crate::{discovery, files};
use nbp_core::buffer::SpanStats;
use nbp_core::Calibration;
use std::time::Duration;
use tokio::sync::Mutex;

/// samples handed to the trigger script per block, at most
const MAX_SCRIPT_SAMPLES: usize = 1_000_000;

/// A scan for the script trigger (`script_scan`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReq {
    pub python: String,
    pub code: String,
    pub t0: f64,
    pub t1: f64,
    pub max: usize,
    /// base64 of a sample block from the UI's pipeline; none: this backend's own
    pub block: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(serde::Serialize, Default)]
pub struct ScanResult {
    /// where the next scan starts, s
    pub next: f64,
    /// samples scanned
    pub n: usize,
    /// triggers found
    pub found: usize,
    /// they, when the block came from the UI (it puts them in its pipeline)
    pub events: Vec<crate::script::Found>,
    pub out: String,
    pub error: Option<String>,
    pub ms: f64,
}

/// What a script check found (`script_check`).
#[derive(serde::Serialize, Default)]
pub struct CheckResult {
    /// Python's version: it loaded
    pub info: Option<crate::script::ScriptInfo>,
    /// why it didn't load, or the traceback of the dry run
    pub error: Option<String>,
    /// samples of the dry run (0: none to run it on), and the time they cover, s
    pub n: usize,
    pub span: f64,
    /// triggers it found there, the first of them
    pub found: usize,
    pub events: Vec<crate::script::Found>,
    pub out: String,
    pub ms: f64,
}

#[derive(Default)]
pub struct AppState {
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
    /// the script trigger's Python
    script: std::sync::Arc<std::sync::Mutex<crate::script::ScriptHost>>,
    /// a second one, for checking a script without touching the running one's state
    script_check: std::sync::Arc<std::sync::Mutex<crate::script::ScriptHost>>,
}

#[derive(serde::Serialize)]
pub struct DiskStatus {
    /// bytes free where the files go
    free: u64,
    recording_error: Option<String>,
    logging_error: Option<String>,
    export_error: Option<String>,
    /// the export file's size so far
    export_bytes: u64,
    dropped: u64,
}

impl AppState {
    fn pipe(&self) -> Option<SharedPipeline> {
        self.pipe.lock().unwrap().clone()
    }


    pub async fn list_usb(&self) -> Result<Vec<link::usb::UsbDev>, String> {
        tokio::task::spawn_blocking(link::usb::list).await.map_err(|e| e.to_string())?
    }

    pub async fn discover(&self, timeout_ms: Option<u64>) -> Result<Vec<discovery::NetDev>, String> {
        discovery::discover(Duration::from_millis(timeout_ms.unwrap_or(600))).await
    }

    /// Opens `target`; Meta and the close event come on `events`.
    pub async fn connect(&self, target: Target, events: Events) -> Result<String, String> {
        let mut slot = self.conn.lock().await;
        slot.take(); // drop (and stop) a previous connection first
        self.pipe.lock().unwrap().take();
        let samples = self.buffer_samples.lock().unwrap().unwrap_or(nbp_core::pipeline::DEFAULT_BUFFER_SAMPLES);
        let cal = self.cal.lock().unwrap().clone();
        let c = Conn::open(target, events, samples, cal).await?;
        c.pipe.lock().unwrap().set_trigger(self.trigger.lock().unwrap().clone());
        if let Some(iv) = *self.log_interval.lock().unwrap() {
            c.pipe.lock().unwrap().set_log_interval(iv);
        }
        let _ = c.pipe.lock().unwrap().set_math(self.math.lock().unwrap().as_deref());
        let d = c.description.clone();
        *self.pipe.lock().unwrap() = Some(c.pipe.clone());
        *slot = Some(c);
        Ok(d)
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        let _ = self.stop_recording().await;
        let _ = self.stop_logging().await;
        let _ = self.stop_export().await;
        self.conn.lock().await.take();
        self.pipe.lock().unwrap().take();
        Ok(())
    }

    // ---- files --------------------------------------------------------------------

    pub fn recordings_dir(&self) -> Result<String, String> {
        files::recordings_dir().map(|d| d.display().to_string())
    }

    pub fn list_recordings(&self) -> Result<Vec<files::RecFile>, String> {
        Ok(files::list(&files::recordings_dir()?))
    }

    /// Records every sample from now on into the recordings folder; returns the file's path.
    pub async fn start_recording(&self, header: String, from: Option<f64>) -> Result<String, String> {
        if self.recording.lock().unwrap().is_some() {
            return Err("already recording".into());
        }
        let conn = self.conn.lock().await;
        let c = conn.as_ref().ok_or("connect to a device to record")?;
        let path = files::recordings_dir()?.join(format!("{}.{}", files::timestamp(), files::EXT));
        let rec = files::start(path.clone(), &header, from, &c.pipe, &c.rec)?;
        *self.recording.lock().unwrap() = Some(rec);
        Ok(path.display().to_string())
    }

    pub async fn stop_recording(&self) -> Result<Option<files::RecordingDone>, String> {
        let Some(rec) = self.recording.lock().unwrap().take() else { return Ok(None) };
        let conn = self.conn.lock().await;
        let c = conn.as_ref().ok_or("connection gone")?;
        let (pipe, sink) = (c.pipe.clone(), c.rec.clone());
        drop(conn);
        tokio::task::spawn_blocking(move || files::stop(rec, &pipe, &sink)).await.map_err(|e| e.to_string())?.map(Some)
    }

    // ---- logging (the session log to a file) -------------------------------------------

    /// Starts writing the session log to ~/Documents/Nebulosa/<time>.nblog; returns the path.
    pub async fn start_logging(&self, header: String, from: Option<f64>) -> Result<String, String> {
        if self.logging.lock().unwrap().is_some() {
            return Err("already logging".into());
        }
        let conn = self.conn.lock().await;
        let c = conn.as_ref().ok_or("connect to a device to log")?;
        let path = files::recordings_dir()?.join(format!("{}.{}", files::timestamp(), files::LOG_EXT));
        let w = files::start_log(path.clone(), &header, from, &c.pipe, &c.log)?;
        *self.logging.lock().unwrap() = Some(w);
        Ok(path.display().to_string())
    }

    pub async fn stop_logging(&self) -> Result<Option<files::LoggingDone>, String> {
        let Some(w) = self.logging.lock().unwrap().take() else { return Ok(None) };
        let conn = self.conn.lock().await;
        let c = conn.as_ref().ok_or("connection gone")?;
        let (pipe, sink) = (c.pipe.clone(), c.log.clone());
        drop(conn);
        tokio::task::spawn_blocking(move || files::stop_log(w, &pipe, &sink)).await.map_err(|e| e.to_string())?.map(Some)
    }

    pub async fn export_preview(&self, cfg: nbp_core::export::ExportCfg, wall_offset: f64, rows: usize) -> Result<String, String> {
        Ok(self.pipe().map(|p| p.lock().unwrap().export_preview(&cfg, wall_offset, rows.min(50))).unwrap_or_default())
    }

    /// Starts an export in the logger's chosen format into ~/Documents/Nebulosa; returns the path.
    pub async fn start_export(&self, cfg: nbp_core::export::ExportCfg, wall_offset: f64, header: String, from: Option<f64>) -> Result<String, String> {
        if self.exporting.lock().unwrap().is_some() {
            return Err("already exporting".into());
        }
        if cfg.fields.is_empty() {
            return Err("no fields chosen".into());
        }
        let conn = self.conn.lock().await;
        let c = conn.as_ref().ok_or("connect to a device to log")?;
        let path = files::recordings_dir()?.join(format!("{}.{}", files::timestamp(), files::export_ext(&cfg.format)));
        let w = files::start_export(path.clone(), cfg, wall_offset, &header, from, &c.pipe, &c.export)?;
        *self.exporting.lock().unwrap() = Some(w);
        Ok(path.display().to_string())
    }

    pub async fn stop_export(&self) -> Result<Option<files::ExportDone>, String> {
        let Some(w) = self.exporting.lock().unwrap().take() else { return Ok(None) };
        let conn = self.conn.lock().await;
        let c = conn.as_ref().ok_or("connection gone")?;
        let (pipe, sink) = (c.pipe.clone(), c.export.clone());
        drop(conn);
        tokio::task::spawn_blocking(move || files::stop_export(w, &pipe, &sink)).await.map_err(|e| e.to_string())?.map(Some)
    }

    /// The session log's interval, s; it starts over.
    pub async fn set_log_interval(&self, interval: f64) -> Result<(), String> {
        let iv = interval.clamp(1e-3, 3600.0);
        *self.log_interval.lock().unwrap() = Some(iv);
        if let Some(p) = self.pipe() {
            p.lock().unwrap().set_log_interval(iv);
        }
        Ok(())
    }

    pub async fn clear_session(&self) -> Result<(), String> {
        if let Some(p) = self.pipe() {
            p.lock().unwrap().clear_session();
        }
        Ok(())
    }

    pub async fn session_lod(&self, t0: f64, t1: f64, bins: usize) -> Result<Vec<u8>, String> {
        let v = match self.pipe() {
            Some(p) => p.lock().unwrap().session_lod(t0, t1, bins.min(8192)),
            None => vec![3.0, 0.0, 18.0, 0.0],
        };
        Ok(v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>())
    }

    pub async fn session_stats(&self, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
        Ok(self.pipe().map(|p| p.lock().unwrap().session_stats(t0, t1)))
    }


    /// For the UI's watchdog: free space, and whether a writer failed or fell behind.
    pub fn disk_status(&self) -> Result<DiskStatus, String> {
        let dir = files::recordings_dir()?;
        let err = |w: &std::sync::Mutex<Option<files::Writer>>| w.lock().unwrap().as_ref().and_then(|w| w.error.lock().unwrap().clone());
        let dropped = [&self.recording, &self.logging]
            .iter()
            .filter_map(|w| w.lock().unwrap().as_ref().map(|w| w.dropped.load(std::sync::atomic::Ordering::Relaxed)))
            .sum::<u64>()
            + self.exporting.lock().unwrap().as_ref().map_or(0, |w| w.dropped.load(std::sync::atomic::Ordering::Relaxed));
        let export_error = self.exporting.lock().unwrap().as_ref().and_then(|w| w.error.lock().unwrap().clone());
        let export_bytes = self.exporting.lock().unwrap().as_ref().and_then(|w| std::fs::metadata(&w.path).ok()).map_or(0, |m| m.len());
        Ok(DiskStatus {
            free: files::free_space(&dir),
            recording_error: err(&self.recording),
            logging_error: err(&self.logging),
            export_error,
            export_bytes,
            dropped,
        })
    }

    /// Opens a session log file (.nblog) for viewing; returns its header (JSON).
    pub async fn open_log(&self, path: String, events: Events) -> Result<String, String> {
        let _ = self.stop_recording().await;
        let _ = self.stop_logging().await;
        self.conn.lock().await.take();
        let (p, header) = tokio::task::spawn_blocking(move || files::open_log(&path, &events)).await.map_err(|e| e.to_string())??;
        *self.pipe.lock().unwrap() = Some(std::sync::Arc::new(std::sync::Mutex::new(p)));
        Ok(header)
    }

    /// Opens a recording for playback (instead of a device); returns its header (JSON).
    pub async fn open_recording(&self, path: String, events: Events) -> Result<String, String> {
        let _ = self.stop_recording().await;
        self.conn.lock().await.take();
        let math = self.math.lock().unwrap().clone();
        let (mut p, header) = tokio::task::spawn_blocking(move || files::open(&path, &events)).await.map_err(|e| e.to_string())??;
        let _ = p.set_math(math.as_deref());
        *self.pipe.lock().unwrap() = Some(std::sync::Arc::new(std::sync::Mutex::new(p)));
        Ok(header)
    }

    /// Every sample in [t0, t1) to a CSV file; returns the rows written.
    pub async fn export_csv(&self, t0: f64, t1: f64, path: String) -> Result<u64, String> {
        let p = self.pipe().ok_or("no data")?;
        tokio::task::spawn_blocking(move || files::export_csv(&p, t0, t1, &path)).await.map_err(|e| e.to_string())?
    }

    pub async fn count_samples(&self, t0: f64, t1: f64) -> Result<u64, String> {
        Ok(self.pipe().map(|p| p.lock().unwrap().count_samples(t0, t1)).unwrap_or(0))
    }

    /// Writes bytes (a PNG) to a file the user picked.
    pub async fn save_bytes(&self, path: String, bytes: Vec<u8>) -> Result<(), String> {
        std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))
    }

    pub async fn request(&self, cmd: u8, payload: Vec<u8>, timeout_ms: Option<u64>) -> Result<Reply, String> {
        let mut slot = self.conn.lock().await;
        let c = slot.as_mut().ok_or("not connected")?;
        c.request(cmd, &payload, Duration::from_millis(timeout_ms.unwrap_or(1500))).await
    }

    /// Samples the buffer holds; resizing drops what it holds.
    pub async fn set_buffer(&self, samples: usize) -> Result<(), String> {
        *self.buffer_samples.lock().unwrap() = Some(samples);
        if let Some(p) = self.pipe() {
            p.lock().unwrap().set_buffer_samples(samples);
        }
        Ok(())
    }

    pub async fn set_calibration(&self, cal: Calibration) -> Result<(), String> {
        *self.cal.lock().unwrap() = cal.clone();
        if let Some(p) = self.pipe() {
            p.lock().unwrap().set_calibration(cal);
        }
        Ok(())
    }

    pub async fn set_trigger(&self, cfg: nbp_core::trigger::TriggerCfg) -> Result<(), String> {
        *self.trigger.lock().unwrap() = cfg.clone();
        if let Some(p) = self.pipe() {
            p.lock().unwrap().set_trigger(cfg);
        }
        Ok(())
    }

    /// The math channel: an expression (see nbp-core math.rs) or none. Errors say what doesn't parse.
    pub async fn set_math(&self, expr: Option<String>) -> Result<(), String> {
        if let Some(e) = &expr {
            nbp_core::math::MathProgram::compile(e)?;
        }
        *self.math.lock().unwrap() = expr.clone();
        if let Some(p) = self.pipe() {
            // computing the buffer again can take a moment: off the async threads
            tokio::task::spawn_blocking(move || p.lock().unwrap().set_math(expr.as_deref())).await.map_err(|e| e.to_string())??;
        }
        Ok(())
    }

    /// Analysis over [t0, t1): the samples are copied out under the lock, analysed outside it.
    pub async fn analyze(&self, t0: f64, t1: f64, req: nbp_core::analysis::Request) -> Result<nbp_core::analysis::Result, String> {
        let p = self.pipe().ok_or("no data")?;
        tokio::task::spawn_blocking(move || {
            let x = p.lock().unwrap().extract(t0, t1);
            nbp_core::analysis::run(&x, &req)
        })
        .await
        .map_err(|e| e.to_string())
    }

    // ---- side-channel campaign -------------------------------------------------------

    pub async fn sca_new(&self, cfg: nbp_core::sca::Config) -> Result<(), String> {
        *self.sca.lock().unwrap() = Some(nbp_core::sca::Campaign::new(cfg)?);
        Ok(())
    }

    /// [t0, t1) of the buffer as one more trace: the traces so far.
    pub async fn sca_add(&self, t0: f64, t1: f64, meta: nbp_core::sca::TraceMeta) -> Result<usize, String> {
        let x = self.pipe().ok_or("no data")?.lock().unwrap().extract(t0, t1);
        self.sca.lock().unwrap().as_mut().ok_or("no campaign")?.add(&x, meta)
    }

    pub async fn sca_info(&self) -> Result<Option<nbp_core::sca::Info>, String> {
        Ok(self.sca.lock().unwrap().as_ref().map(|c| c.info()))
    }

    pub async fn sca_align(&self, a: usize, b: usize, max_shift: usize) -> Result<nbp_core::sca::AlignResult, String> {
        let sca = self.sca.clone();
        tokio::task::spawn_blocking(move || sca.lock().unwrap().as_mut().ok_or("no campaign".to_string())?.align(a, b, max_shift))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn sca_clear_align(&self) -> Result<(), String> {
        if let Some(c) = self.sca.lock().unwrap().as_mut() {
            c.clear_align();
        }
        Ok(())
    }

    pub async fn sca_query(&self, req: nbp_core::sca::Request) -> Result<nbp_core::sca::Result, String> {
        let sca = self.sca.clone();
        tokio::task::spawn_blocking(move || sca.lock().unwrap().as_ref().map(|c| c.query(&req)).ok_or("no campaign".to_string()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn sca_meta(&self) -> Result<Vec<nbp_core::sca::TraceMeta>, String> {
        Ok(self.sca.lock().unwrap().as_ref().map(|c| c.meta().to_vec()).unwrap_or_default())
    }

    /// The campaign as an .npz, raw bytes.
    pub async fn sca_export(&self) -> Result<Vec<u8>, String> {
        let sca = self.sca.clone();
        tokio::task::spawn_blocking(move || sca.lock().unwrap().as_ref().map(|c| c.export_npz()).ok_or("no campaign".to_string()))
            .await
            .map_err(|e| e.to_string())?
    }

    /// The campaign as a flat .nbsca binary (save/reload).
    pub async fn sca_save(&self) -> Result<Vec<u8>, String> {
        let sca = self.sca.clone();
        tokio::task::spawn_blocking(move || sca.lock().unwrap().as_ref().map(|c| c.export_raw()).ok_or("no campaign".to_string()))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Load a campaign from .nbsca bytes; returns its info.
    pub async fn sca_load(&self, bytes: Vec<u8>) -> Result<nbp_core::sca::Info, String> {
        let c = nbp_core::sca::Campaign::from_raw(&bytes)?;
        let info = c.info();
        *self.sca.lock().unwrap() = Some(c);
        Ok(info)
    }

    /// Trigger events with seq ≥ since, at most max.
    pub async fn query_events(&self, since: u64, max: usize) -> Result<Vec<nbp_core::trigger::Event>, String> {
        Ok(self.pipe().map(|p| p.lock().unwrap().events_since(since, max.min(10_000))).unwrap_or_default())
    }

    pub async fn clear_events(&self) -> Result<(), String> {
        if let Some(p) = self.pipe() {
            p.lock().unwrap().clear_events();
        }
        Ok(())
    }

    /// Events the script trigger found (on a pipeline elsewhere, the browser's): into the event log.
    pub async fn inject_events(&self, events: Vec<nbp_core::trigger::Event>) -> Result<(), String> {
        if let Some(p) = self.pipe() {
            p.lock().unwrap().inject_events(events);
        }
        Ok(())
    }

    /// Starts the trigger script (Python `python`, or python3 / python): its Python and numpy versions.
    pub async fn script_load(&self, python: String, code: String) -> Result<crate::script::ScriptInfo, String> {
        let h = self.script.clone();
        tokio::task::spawn_blocking(move || h.lock().unwrap().load(&python, &code)).await.map_err(|e| e.to_string())?
    }

    /// The trigger script on the samples in [t0, t1) (at most max): `block` (base64, from a
    /// pipeline in the UI) and its events come back; without it, this backend's own pipeline's
    /// samples, and its events go straight into its log.
    pub async fn script_scan(&self, req: ScanReq) -> Result<ScanResult, String> {
        let own = req.block.is_none();
        let block = match &req.block {
            Some(b) => crate::script::unbase64(b)?,
            None => match self.pipe() {
                Some(p) => p.lock().unwrap().sample_block(req.t0, req.t1, req.max.min(MAX_SCRIPT_SAMPLES)),
                None => return Ok(ScanResult { next: req.t0, ..Default::default() }),
            },
        };
        let (end, n) = crate::script::block_end(&block).ok_or("not a sample block")?;
        let mut r = ScanResult { next: if n > 0 { end } else { req.t0 }, n, ..Default::default() };
        if n == 0 {
            return Ok(r);
        }
        let timeout = Duration::from_millis(req.timeout_ms.unwrap_or(5000).clamp(50, 60_000));
        let h = self.script.clone();
        let (python, code) = (req.python, req.code);
        let scan = tokio::task::spawn_blocking(move || h.lock().unwrap().scan(&python, &code, &block, timeout)).await.map_err(|e| e.to_string())??;
        r.out = scan.out;
        r.error = scan.error;
        r.ms = scan.ms;
        r.found = scan.events.len();
        if own {
            if let Some(p) = self.pipe() {
                p.lock().unwrap().inject_events(scan.events.into_iter().map(|e| e.event()).collect());
            }
        } else {
            r.events = scan.events;
        }
        Ok(r)
    }

    /// A script checked: it loads (Python's version, or the error), and a dry run on the
    /// samples in [t0, t1) (`block`, or this backend's own) says what it would find there.
    /// Its own Python, started fresh: the running script's state isn't touched.
    pub async fn script_check(&self, req: ScanReq) -> Result<CheckResult, String> {
        let h = self.script_check.clone();
        let (python, code) = (req.python.clone(), req.code.clone());
        let loaded = tokio::task::spawn_blocking(move || {
            let mut h = h.lock().unwrap();
            h.stop();
            h.load(&python, &code)
        })
        .await
        .map_err(|e| e.to_string())?;
        let info = match loaded {
            Ok(i) => i,
            Err(e) => return Ok(CheckResult { error: Some(e), ..Default::default() }),
        };
        let block = match &req.block {
            Some(b) => crate::script::unbase64(b)?,
            None => match self.pipe() {
                Some(p) => p.lock().unwrap().sample_block(req.t0, req.t1, req.max.min(MAX_SCRIPT_SAMPLES)),
                None => Vec::new(),
            },
        };
        let (end, n) = crate::script::block_end(&block).unwrap_or((f64::NAN, 0));
        let first = block.get(20..28).map_or(f64::NAN, |b| f64::from_le_bytes(b.try_into().unwrap()));
        let span = if n > 0 { end - first } else { 0.0 };
        let mut r = CheckResult { info: Some(info), n, span, ..Default::default() };
        if n == 0 {
            return Ok(r);
        }
        let timeout = Duration::from_millis(req.timeout_ms.unwrap_or(10_000).clamp(50, 60_000));
        let h = self.script_check.clone();
        let (python, code) = (req.python, req.code);
        let scan = tokio::task::spawn_blocking(move || {
            let mut h = h.lock().unwrap();
            let s = h.scan(&python, &code, &block, timeout);
            h.stop();
            s
        })
        .await
        .map_err(|e| e.to_string())?;
        match scan {
            Ok(s) => {
                r.found = s.events.len();
                r.events = s.events.into_iter().take(20).collect();
                r.out = s.out;
                r.error = s.error;
                r.ms = s.ms;
            }
            Err(e) => r.error = Some(e),
        }
        Ok(r)
    }

    /// The trigger starts over: the script's reset(), if it has one.
    pub async fn script_reset(&self, python: String, code: String) -> Result<(), String> {
        let h = self.script.clone();
        tokio::task::spawn_blocking(move || h.lock().unwrap().reset(&python, &code)).await.map_err(|e| e.to_string())?
    }

    pub async fn script_stop(&self) -> Result<(), String> {
        self.script.lock().unwrap().stop();
        Ok(())
    }

    /// [t0, t1] in `bins` columns: SampleBuffer::lod's flat f64 layout, raw bytes.
    pub async fn query_lod(&self, t0: f64, t1: f64, bins: usize) -> Result<Vec<u8>, String> {
        let v = match self.pipe() {
            Some(p) => p.lock().unwrap().lod(t0, t1, bins.min(8192)),
            None => vec![2.0, 0.0, 14.0, 0.0],
        };
        Ok(v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>())
    }

    pub async fn query_stats(&self, t0: f64, t1: f64) -> Result<Option<SpanStats>, String> {
        Ok(self.pipe().map(|p| p.lock().unwrap().stats(t0, t1)))
    }

    pub async fn query_value(&self, t: f64) -> Result<Option<(f64, f64, u8, f64)>, String> {
        Ok(self.pipe().and_then(|p| p.lock().unwrap().value_at(t)))
    }

}

/// A command's answer: JSON, or raw bytes (the plot data).
pub enum Out {
    Json(serde_json::Value),
    Bytes(Vec<u8>),
}

fn json<T: serde::Serialize>(v: T) -> Out {
    Out::Json(serde_json::to_value(v).unwrap_or(serde_json::Value::Null))
}

/// An argument by its JS name (camelCase, as Tauri names them); absent is null.
fn arg<T: serde::de::DeserializeOwned>(a: &serde_json::Value, k: &str) -> Result<T, String> {
    serde_json::from_value(a.get(k).cloned().unwrap_or(serde_json::Value::Null)).map_err(|e| format!("{k}: {e}"))
}

/// A command by name, its arguments as the UI sent them (the WebSocket shell).
/// `ev`: where the command's events go, for the commands that take a channel.
pub async fn dispatch(st: &AppState, ser: &crate::serial::SerialState, cmd: &str, a: &serde_json::Value, ev: Option<Events>) -> Result<Out, String> {
    Ok(match cmd {
        "list_usb" => json(st.list_usb().await?),
        "discover" => json(st.discover(arg(a, "timeoutMs")?).await?),
        "connect" => json(st.connect(arg(a, "target")?, ev.clone().ok_or("no channel")?).await?),
        "disconnect" => json(st.disconnect().await?),
        "recordings_dir" => json(st.recordings_dir()?),
        "list_recordings" => json(st.list_recordings()?),
        "start_recording" => json(st.start_recording(arg(a, "header")?, arg(a, "from")?).await?),
        "stop_recording" => json(st.stop_recording().await?),
        "start_logging" => json(st.start_logging(arg(a, "header")?, arg(a, "from")?).await?),
        "stop_logging" => json(st.stop_logging().await?),
        "export_preview" => json(st.export_preview(arg(a, "cfg")?, arg(a, "wallOffset")?, arg(a, "rows")?).await?),
        "start_export" => json(st.start_export(arg(a, "cfg")?, arg(a, "wallOffset")?, arg(a, "header")?, arg(a, "from")?).await?),
        "stop_export" => json(st.stop_export().await?),
        "set_log_interval" => json(st.set_log_interval(arg(a, "interval")?).await?),
        "clear_session" => json(st.clear_session().await?),
        "session_lod" => Out::Bytes(st.session_lod(arg(a, "t0")?, arg(a, "t1")?, arg(a, "bins")?).await?),
        "session_stats" => json(st.session_stats(arg(a, "t0")?, arg(a, "t1")?).await?),
        "disk_status" => json(st.disk_status()?),
        "open_log" => json(st.open_log(arg(a, "path")?, ev.clone().ok_or("no channel")?).await?),
        "open_recording" => json(st.open_recording(arg(a, "path")?, ev.clone().ok_or("no channel")?).await?),
        "export_csv" => json(st.export_csv(arg(a, "t0")?, arg(a, "t1")?, arg(a, "path")?).await?),
        "count_samples" => json(st.count_samples(arg(a, "t0")?, arg(a, "t1")?).await?),
        "save_bytes" => json(st.save_bytes(arg(a, "path")?, arg(a, "bytes")?).await?),
        "request" => json(st.request(arg(a, "cmd")?, arg(a, "payload")?, arg(a, "timeoutMs")?).await?),
        "set_buffer" => json(st.set_buffer(arg(a, "samples")?).await?),
        "set_calibration" => json(st.set_calibration(arg(a, "cal")?).await?),
        "set_trigger" => json(st.set_trigger(arg(a, "cfg")?).await?),
        "set_math" => json(st.set_math(arg(a, "expr")?).await?),
        "analyze" => json(st.analyze(arg(a, "t0")?, arg(a, "t1")?, arg(a, "req")?).await?),
        "sca_new" => json(st.sca_new(arg(a, "cfg")?).await?),
        "sca_add" => json(st.sca_add(arg(a, "t0")?, arg(a, "t1")?, arg(a, "meta")?).await?),
        "sca_info" => json(st.sca_info().await?),
        "sca_align" => json(st.sca_align(arg(a, "a")?, arg(a, "b")?, arg(a, "maxShift")?).await?),
        "sca_clear_align" => json(st.sca_clear_align().await?),
        "sca_query" => json(st.sca_query(arg(a, "req")?).await?),
        "sca_meta" => json(st.sca_meta().await?),
        "sca_export" => Out::Bytes(st.sca_export().await?),
        "sca_save" => Out::Bytes(st.sca_save().await?),
        "sca_load" => json(st.sca_load(arg(a, "bytes")?).await?),
        "query_events" => json(st.query_events(arg(a, "since")?, arg(a, "max")?).await?),
        "clear_events" => json(st.clear_events().await?),
        "inject_events" => json(st.inject_events(arg(a, "events")?).await?),
        "script_load" => json(st.script_load(arg(a, "python")?, arg(a, "code")?).await?),
        "script_scan" => json(st.script_scan(arg(a, "req")?).await?),
        "script_check" => json(st.script_check(arg(a, "req")?).await?),
        "script_reset" => json(st.script_reset(arg(a, "python")?, arg(a, "code")?).await?),
        "script_stop" => json(st.script_stop().await?),
        "query_lod" => Out::Bytes(st.query_lod(arg(a, "t0")?, arg(a, "t1")?, arg(a, "bins")?).await?),
        "query_stats" => json(st.query_stats(arg(a, "t0")?, arg(a, "t1")?).await?),
        "query_value" => json(st.query_value(arg(a, "t")?).await?),
        "serial_open" => json(ser.serial_open(arg(a, "path")?, arg(a, "cfg")?, ev.clone().ok_or("no channel")?).await?),
        "serial_write" => json(ser.serial_write(arg(a, "data")?).await?),
        "serial_signals" => json(ser.serial_signals(arg(a, "rts")?, arg(a, "dtr")?).await?),
        "serial_close" => json(ser.serial_close().await?),
        "serial_list" => json(crate::serial::serial_list().await?),
        _ => return Err(format!("unknown command {cmd}")),
    })
}

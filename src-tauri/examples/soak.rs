//! Soak test of the native backend, no window: connects to a device or to
//! nebulosa-sim, streams, records (.nbrec), logs (.nblog) and exports (CSV
//! of every sample, then HDF5 of the summaries) at once, watches drops and
//! memory, then reads every file back and checks it.
//!
//!   cargo run --release -p nebulosa-companion --example soak -- [target] [seconds] [log2n] [out dir]
//!   target: 127.0.0.1 (TCP, default), host:port, or usb (the first Nebulosa on USB)
//!
//! Exit status 0 when every check passes.

use nbp_core::export::{Data, ExportCfg, Field, Format, Stat, TimeFmt, What};
use nbp_core::{msg, Calibration};
use nebulosa_companion_lib::files;
use nebulosa_companion_lib::link::{Conn, Target};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};

fn rss_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    s.split_whitespace().nth(1).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0) * 4096.0 / 1e6
}

fn field(what: What, label: &str, scale: f64, decimals: u8) -> Field {
    Field { what, stat: Stat::Mean, label: label.into(), scale, decimals }
}

struct Check {
    fails: Vec<String>,
}

impl Check {
    fn ok(&mut self, cond: bool, what: &str) {
        println!("  {} {what}", if cond { "PASS" } else { "FAIL" });
        if !cond {
            self.fails.push(what.to_string());
        }
    }
}

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let target_s = a.first().cloned().unwrap_or("127.0.0.1".into());
    let secs: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let log2n: u8 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let dir = std::path::PathBuf::from(a.get(3).cloned().unwrap_or_else(|| std::env::temp_dir().join("nebulosa-soak").display().to_string()));
    std::fs::create_dir_all(&dir).unwrap();

    let target = if target_s == "usb" {
        let devs = nebulosa_companion_lib::link::usb::list().expect("USB list");
        let d = devs.first().expect("no Nebulosa on USB");
        println!("USB: {}", d.label);
        Target::Usb { id: d.id.clone() }
    } else {
        let (host, port) = target_s.split_once(':').map_or((target_s.clone(), None), |(h, p)| (h.to_string(), p.parse().ok()));
        Target::Tcp { host, port }
    };

    // the UI's channel, here a callback counting Meta events (the harness reads
    // the pipeline directly) and reporting a closed connection
    let metas = Arc::new(Mutex::new(0u64));
    let m2 = metas.clone();
    let events = Channel::new(move |b: InvokeResponseBody| {
        if let InvokeResponseBody::Json(s) = b {
            if s.contains("\"event\":\"meta\"") {
                *m2.lock().unwrap() += 1;
            } else if s.contains("\"event\":\"closed\"") {
                eprintln!("connection closed: {s}");
            }
        }
        Ok(())
    });

    let mut c = Conn::open(target, events, 1 << 24, Calibration::default()).await.expect("connect");
    println!("connected: {}", c.description);
    let t = Duration::from_millis(1500);
    let info = c.request(msg::cmd::GET_INFO, &[], t).await.expect("GET_INFO");
    let info = msg::Info::parse(&info.payload).expect("info");
    println!("firmware {} FPGA {:08X} link {}", info.fw_version_str, info.fpga_id, info.link_name);
    c.request(msg::cmd::SET_SOURCE, &msg::enc_source(msg::SRC_STUB_RANDOM), t).await.expect("source");
    if info.caps & msg::caps::DECIM != 0 {
        c.request(msg::cmd::SET_DECIMATION, &msg::enc_decim(log2n, 0), t).await.expect("decimation");
    }
    c.pipe.lock().unwrap().set_math(Some("P*1000")).unwrap();
    c.request(msg::cmd::STREAM_START, &msg::enc_stream_start(msg::START_FLUSH), t).await.expect("start");
    tokio::time::sleep(Duration::from_millis(500)).await;

    let stamp = files::timestamp();
    let rec_path = dir.join(format!("{stamp}.nbrec"));
    let log_path = dir.join(format!("{stamp}.nblog"));
    let csv_path = dir.join(format!("{stamp}.csv"));
    let h5_path = dir.join(format!("{stamp}.h5"));
    let header = "{\"soak\":true}";
    let rec = files::start(rec_path.clone(), header, None, &c.pipe, &c.rec).expect("record");
    let log = files::start_log(log_path.clone(), header, None, &c.pipe, &c.log).expect("log");
    let raw_cfg = ExportCfg {
        data: Data::Raw,
        format: Format::Csv { separator: ",".into(), decimal_comma: false, header: true },
        time: TimeFmt::Relative,
        fields: vec![
            field(What::Time, "t", 1.0, 7),
            field(What::I, "I_uA", 1e6, 3),
            field(What::V, "V", 1.0, 4),
            field(What::M, "P_mW", 1.0, 4),
        ],
    };
    let csv = files::start_export(csv_path.clone(), raw_cfg, 0.0, header, None, &c.pipe, &c.export).expect("csv export");

    let csv_secs = (secs / 3).clamp(2, 10);
    let start = Instant::now();
    let base = c.pipe.lock().unwrap().meta();
    let (mut rss_min, mut rss_max) = (f64::MAX, 0f64);
    let mut csv = Some(csv);
    let mut h5 = None;
    let mut csv_rows_expect = 0u64;
    let mut h5_rows_expect = 0u64;
    let mut csv_start_samples = base.run.n();
    let mut h5_start_entries = 0u64;
    for s in 1..=secs {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let m = c.pipe.lock().unwrap().meta();
        if s == csv_secs {
            // switch: CSV of every sample off, HDF5 of the summaries on
            let done = files::stop_export(csv.take().unwrap(), &c.pipe, &c.export).expect("csv stop");
            csv_rows_expect = done.rows;
            println!("  csv: {} rows, {:.0} MB in {} s", done.rows, done.bytes as f64 / 1e6, s);
            let _ = csv_start_samples;
            csv_start_samples = 0;
            let h5_cfg = ExportCfg {
                data: Data::Summary,
                format: Format::Hdf5,
                time: TimeFmt::Unix,
                fields: vec![field(What::Time, "t", 1.0, 6), field(What::I, "I", 1.0, 9), field(What::Charge, "Q", 1.0, 9)],
            };
            h5_start_entries = m.session.entries;
            h5 = Some(files::start_export(h5_path.clone(), h5_cfg, 1.7e9, header, None, &c.pipe, &c.export).expect("h5 export"));
        }
        if s >= 5 {
            let r = rss_mb();
            rss_min = rss_min.min(r);
            rss_max = rss_max.max(r);
        }
        if s % 5 == 0 || s == secs {
            println!(
                "{s:4} s  {:.2} MS/s  lost {} stalls {} malformed {}  rec {:.0} MB  log {} entries  RSS {:.0} MB",
                (m.run.n() - base.run.n()) as f64 / start.elapsed().as_secs_f64() / 1e6,
                m.stats.lost_blocks,
                m.stats.stall_flags,
                m.stats.stream.malformed,
                m.recording.bytes as f64 / 1e6,
                m.logging.entries,
                rss_mb()
            );
        }
    }
    let end = c.pipe.lock().unwrap().meta();
    if let Some(h) = h5.take() {
        let done = files::stop_export(h, &c.pipe, &c.export).expect("h5 stop");
        h5_rows_expect = done.rows;
        let _ = h5_start_entries;
    }
    let rec_done = files::stop(rec, &c.pipe, &c.rec).expect("rec stop");
    let log_done = files::stop_log(log, &c.pipe, &c.log).expect("log stop");
    c.request(msg::cmd::STREAM_STOP, &[], t).await.expect("stop");
    drop(c);

    println!("\nchecks");
    let mut ck = Check { fails: Vec::new() };
    let rate = (end.run.n() - base.run.n()) as f64 / secs as f64;
    let expect_rate = info.base_rate_hz as f64 / (1u64 << log2n) as f64;
    ck.ok(rate > expect_rate * 0.97, &format!("rate {:.3} MS/s of {:.3}", rate / 1e6, expect_rate / 1e6));
    let n_meta = *metas.lock().unwrap();
    ck.ok(n_meta as f64 > secs as f64 * 10.0, &format!("Meta to the UI: {n_meta} events (~25 / s)"));
    ck.ok(end.stats.lost_blocks == 0, &format!("no lost blocks ({})", end.stats.lost_blocks));
    ck.ok(end.stats.stall_flags == 0, &format!("no stalls ({})", end.stats.stall_flags));
    ck.ok(end.stats.stream.malformed == 0 && end.stats.stream.seq_errors == 0, "no malformed words, no FPGA sequence errors");
    ck.ok(rss_max - rss_min < 50.0, &format!("memory steady: RSS {rss_min:.0}..{rss_max:.0} MB after 5 s"));

    // .nbrec: every sample recorded, and it reads back
    let rec_bytes = std::fs::read(&rec_path).unwrap();
    let p = nbp_core::Pipeline::from_recording(&rec_bytes).expect("recording reads");
    let rm = p.meta();
    ck.ok(rm.run.n() == rec_done.status.samples, &format!(".nbrec: {} samples read back of {} written", rm.run.n(), rec_done.status.samples));
    ck.ok(rec_done.status.samples as f64 > rate * (secs as f64 - 1.0), ".nbrec covers the run");
    // .nblog
    let lf = nbp_core::session::LogFile::parse(&std::fs::read(&log_path).unwrap()).expect("log reads");
    ck.ok(!lf.truncated && lf.entries.len() as u64 == log_done.entries, &format!(".nblog: {} entries, complete", lf.entries.len()));
    ck.ok((lf.entries.len() as f64 - secs as f64 * 10.0).abs() <= 12.0, ".nblog: ~10 entries a second");
    // CSV of every sample
    let csv_text = std::fs::read_to_string(&csv_path).unwrap();
    let lines = csv_text.lines().count() as u64;
    ck.ok(lines == csv_rows_expect + 1, &format!("CSV: {} rows + header", lines - 1));
    let last_line = csv_text.lines().last().unwrap_or("");
    let cols: Vec<&str> = last_line.split(',').collect();
    ck.ok(cols.len() == 4 && cols.iter().all(|c| c.parse::<f64>().is_ok()), &format!("CSV: last row parses ({last_line})"));
    ck.ok(csv_rows_expect as f64 > rate * (csv_secs as f64 - 1.5), "CSV: every sample of its phase");
    // HDF5
    let h = hdf5_pure::File::open(&h5_path).expect("h5 opens");
    let q = h.dataset("Q").unwrap().read_f64().unwrap();
    ck.ok(q.len() as u64 == h5_rows_expect && h5_rows_expect > 0, &format!("HDF5: {} rows", q.len()));
    ck.ok(q.windows(2).all(|w| w[1] >= w[0]), "HDF5: charge non-decreasing");

    println!("\nfiles in {}", dir.display());
    if ck.fails.is_empty() {
        println!("ALL PASS");
    } else {
        println!("{} FAILED: {:?}", ck.fails.len(), ck.fails);
        std::process::exit(1);
    }
}

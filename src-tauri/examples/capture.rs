//! Triggered capture on the native backend, no window: streams from a device
//! or nebulosa-sim, lets the trigger fire, then starts every kind of file the
//! Logger writes *from before the trigger* (as the app's ◎ Arm does) and
//! checks each one: it starts where it should, every sample once, in order,
//! and goes on after the trigger.
//!
//!   cargo run --release -p nebulosa-companion --example capture -- [target] [out dir]
//!   target: 127.0.0.1 (TCP, default), host:port, or usb
//!
//! Exit status 0 when every check passes.

use nbp_core::export::{Data, ExportCfg, Field, Format, Stat, TimeFmt, What};
use nbp_core::{msg, Calibration};
use nebulosa_companion_lib::files;
use nebulosa_companion_lib::link::{Conn, Target};
use std::time::Duration;
use nebulosa_companion_lib::events::Events;

const PRE: f64 = 3.0;
const POST: f64 = 2.0;

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

fn field(what: What, label: &str, scale: f64, decimals: u8) -> Field {
    Field { what, stat: Stat::Mean, label: label.into(), scale, decimals }
}

fn csv_cfg(data: Data) -> ExportCfg {
    ExportCfg {
        data,
        format: Format::Csv { separator: ",".into(), decimal_comma: false, header: true },
        time: TimeFmt::Unix,
        fields: vec![field(What::Time, "t", 1.0, 7), field(What::I, "I_uA", 1e6, 3)],
    }
}

/// the first column of a CSV with a header row
fn csv_times(path: &std::path::Path) -> Vec<f64> {
    std::fs::read_to_string(path).unwrap().lines().skip(1).filter_map(|l| l.split(',').next()?.parse().ok()).collect()
}

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let target_s = a.first().cloned().unwrap_or("127.0.0.1".into());
    let dir = std::path::PathBuf::from(a.get(1).cloned().unwrap_or_else(|| std::env::temp_dir().join("nebulosa-capture").display().to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = if target_s == "usb" {
        let d = nebulosa_companion_lib::link::usb::list().expect("USB list").into_iter().next().expect("no Nebulosa on USB");
        Target::Usb { id: d.id }
    } else {
        let (host, port) = target_s.split_once(':').map_or((target_s.clone(), None), |(h, p)| (h.to_string(), p.parse().ok()));
        Target::Tcp { host, port }
    };
    let events = Events::none();
    // 16 Mi samples: 8 s at 2 MS/s, room for the 3 s before
    let mut c = Conn::open(target, events, 1 << 24, Calibration::default()).await.expect("connect");
    println!("connected: {}", c.description);
    let t = Duration::from_millis(1500);
    c.request(msg::cmd::SET_SOURCE, &msg::enc_source(msg::SRC_STUB_RANDOM), t).await.expect("source");
    c.request(msg::cmd::STREAM_START, &msg::enc_stream_start(msg::START_FLUSH), t).await.expect("start");
    tokio::time::sleep(Duration::from_secs(5)).await; // more than PRE in memory

    let pipe = c.pipe.clone();
    let meta = || pipe.lock().unwrap().meta();
    let m = meta();
    let period = m.sample_period;
    let interval = m.session.interval;
    println!("{:.2} MS/s, log interval {} s, {:.1} s in the buffer", 1.0 / period / 1e6, interval, m.t_end - m.t_start);

    // the trigger: a rising edge through the current's mean, at most one a second
    let st = pipe.lock().unwrap().stats(m.t_end - 0.5, m.t_end);
    let cfg: nbp_core::trigger::TriggerCfg = serde_json::from_value(serde_json::json!({
        "enabled": true, "source": "i",
        "kind": { "type": "edge", "slope": "rise", "level": st.m.i.mean },
        "hysteresis": (st.m.i.max - st.m.i.min) * 0.1, "holdoff": 1.0, "filter": 0.0
    }))
    .expect("trigger cfg");
    pipe.lock().unwrap().set_trigger(cfg);

    let mut ck = Check { fails: Vec::new() };
    let header = "{\"capture-test\":true}";

    // ---- every sample (.nbrec), the session log (.nblog), CSV of every sample ----
    // armed: the next event after now
    let armed_count = meta().trigger.count;
    let mut trig_t = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let ev = pipe.lock().unwrap().events_since(armed_count, 1);
        if let Some(e) = ev.first() {
            trig_t = Some(e.t);
            break;
        }
    }
    let Some(trig) = trig_t else {
        println!("no trigger in 5 s");
        std::process::exit(1);
    };
    let heard = meta().t_end;
    let from = trig - PRE;
    println!("\ntriggered at {trig:.6} s (heard {:.0} ms later); capturing from {from:.6} s", (heard - trig) * 1e3);
    let stamp = files::timestamp();
    let rec_path = dir.join(format!("{stamp}.nbrec"));
    let log_path = dir.join(format!("{stamp}.nblog"));
    let csv_path = dir.join(format!("{stamp}-raw.csv"));
    let rec = files::start(rec_path.clone(), header, Some(from), &c.pipe, &c.rec).expect("record");
    let log = files::start_log(log_path.clone(), header, Some(from), &c.pipe, &c.log).expect("log");
    let csv = files::start_export(csv_path.clone(), csv_cfg(Data::Raw), 0.0, header, Some(from), &c.pipe, &c.export).expect("csv");
    while meta().t_end < trig + POST {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let rec_done = files::stop(rec, &c.pipe, &c.rec).expect("rec stop");
    let log_done = files::stop_log(log, &c.pipe, &c.log).expect("log stop");
    let csv_done = files::stop_export(csv, &c.pipe, &c.export).expect("csv stop");
    let end = meta().t_end;
    // the stream's own gaps over the same span (samples that never came): the files may have these, no others
    let mut src_gaps = 0u64;
    let mut prev = f64::NAN;
    pipe.lock().unwrap().for_each_sample(from - period / 2.0, end + period, |t, _, _, _, _| {
        if prev.is_finite() && (t - prev) / period > 1.0 + 1e-3 {
            src_gaps += 1;
            println!("    the stream itself skips at {:.7} s after the capture's start: {:.2} samples", prev - from, (t - prev) / period - 1.0);
        }
        prev = t;
    });
    println!("stopped at {end:.6} s: .nbrec {} samples, .nblog {} entries, CSV {} rows", rec_done.status.samples, log_done.entries, csv_done.rows);

    println!("\nchecks: every sample (.nbrec)");
    let p = nbp_core::Pipeline::from_recording(&std::fs::read(&rec_path).unwrap()).expect("recording reads");
    let (mut first, mut last, mut n, mut gaps, mut repeats) = (f64::NAN, f64::NAN, 0u64, 0u64, 0u64);
    p.for_each_sample(f64::MIN, f64::MAX, |t, _, _, _, _| {
        if n > 0 {
            let k = (t - last) / period;
            if k < 1.0 - 1e-3 {
                repeats += 1;
            } else if k > 1.0 + 1e-3 {
                gaps += 1;
                println!("    gap at {:.7} s after the capture's start: {:.2} samples", last - from, k - 1.0);
            }
        }
        if n == 0 {
            first = t;
        }
        last = t;
        n += 1;
    });
    ck.ok((first - from).abs() <= period * 1.5, &format!("starts {:.1} µs from the capture's start ({PRE} s before the trigger)", (first - from) * 1e6));
    // a gap is the stream's (samples that never arrived); a repeat or a step back would be the file's
    let lost = meta().stats.lost_blocks;
    ck.ok(repeats == 0, &format!("every sample once, in order: {repeats} repeated or out of order in {n}"));
    let _ = lost;
    ck.ok(gaps == src_gaps, &format!("gaps only where the stream has them: {gaps} in the file, {src_gaps} in the stream"));
    ck.ok(n == rec_done.status.samples, &format!("{n} samples read back of {} written", rec_done.status.samples));
    ck.ok(first < trig && last >= trig + POST - 0.05, &format!("holds the trigger, and {:.2} s after it", last - trig));

    println!("\nchecks: the session log (.nblog)");
    let lf = nbp_core::session::LogFile::parse(&std::fs::read(&log_path).unwrap()).expect("log reads");
    let e = &lf.entries;
    ck.ok(!lf.truncated && e.len() as u64 == log_done.entries, &format!("{} entries, complete", e.len()));
    let e0 = e.first().map_or(f64::NAN, |x| x.t0);
    ck.ok(e0 <= from + interval && e0 >= from - interval, &format!("first entry {:.3} s before the trigger (wanted {PRE})", trig - e0));
    let steps_ok = e.windows(2).all(|w| ((w[1].t0 - w[0].t0) / interval - 1.0).abs() < 0.2);
    ck.ok(steps_ok, "one entry per interval, none missing or repeated");
    ck.ok(e.last().is_some_and(|x| x.t0 + x.dur >= trig + POST - 2.0 * interval), "goes on after the trigger");

    println!("\nchecks: CSV of every sample");
    let ts = csv_times(&csv_path);
    ck.ok(ts.len() as u64 == csv_done.rows, &format!("{} rows read back of {}", ts.len(), csv_done.rows));
    let back = ts.windows(2).filter(|w| (w[1] - w[0]) / period < 0.95).count();
    let gaps2 = ts.windows(2).filter(|w| (w[1] - w[0]) / period > 1.05).count() as u64;
    ck.ok(back == 0, &format!("in order, none repeated ({back})"));
    ck.ok(gaps2 == gaps, &format!("the same gaps as the .nbrec ({gaps2})"));
    let span = ts.last().unwrap_or(&0.0) - ts.first().unwrap_or(&0.0);
    ck.ok((span - (end - from)).abs() < 0.1 && span > PRE + POST - 0.05, &format!("spans {span:.3} s ({PRE} before + {POST}+ after)"));

    // ---- per interval, to CSV: the session log's entries before the trigger ----
    println!("\nchecks: CSV per interval");
    let t2 = meta().t_end;
    let from2 = t2 - PRE;
    let csv2 = dir.join(format!("{stamp}-summary.csv"));
    let x = files::start_export(csv2.clone(), csv_cfg(Data::Summary), 0.0, header, Some(from2), &c.pipe, &c.export).expect("csv2");
    while meta().t_end < t2 + 1.0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let d2 = files::stop_export(x, &c.pipe, &c.export).expect("csv2 stop");
    let ts2 = csv_times(&csv2);
    let expect = ((PRE + 1.0) / interval).round() as i64;
    ck.ok((ts2.len() as i64 - expect).abs() <= 3, &format!("{} rows of ~{expect} ({PRE} s before + 1 s after, one per {interval} s)", ts2.len()));
    ck.ok(ts2.first().is_some_and(|t0| (t0 - from2).abs() <= interval * 1.5), "starts at the window's start");
    ck.ok(ts2.windows(2).all(|w| ((w[1] - w[0]) / interval - 1.0).abs() < 0.2), "one row per interval");
    ck.ok(d2.rows as usize == ts2.len(), "rows written = rows read");

    // ---- asking for more than memory holds: from what there is, no harm ----
    println!("\nchecks: further back than the buffer");
    let m3 = meta();
    let r3 = dir.join(format!("{stamp}-far.nbrec"));
    let rec3 = files::start(r3.clone(), header, Some(m3.t_end - 1e6), &c.pipe, &c.rec).expect("far record");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let d3 = files::stop(rec3, &c.pipe, &c.rec).expect("far stop");
    let p3 = nbp_core::Pipeline::from_recording(&std::fs::read(&r3).unwrap()).expect("far reads");
    let mm = p3.meta();
    ck.ok((mm.t_start - m3.t_start).abs() < 0.1, &format!("starts at the oldest sample held ({:.2} s back)", m3.t_end - mm.t_start));
    ck.ok(mm.run.n() == d3.status.samples, "every sample of it reads back");

    c.request(msg::cmd::STREAM_STOP, &[], t).await.ok();
    drop(c);
    println!("\nfiles in {}", dir.display());
    if ck.fails.is_empty() {
        println!("ALL PASS");
    } else {
        println!("{} FAILED: {:?}", ck.fails.len(), ck.fails);
        std::process::exit(1);
    }
}

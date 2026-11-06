//! The script trigger: the user's Python function gets the samples as they
//! come, in blocks, and says where the triggers are (and may label them).
//!
//! One Python process per script (script_runner.py, a JSON object a line
//! over stdin / stdout), started on first use and again when the script or
//! the interpreter changes; the script's globals live as long as it does.

use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RUNNER: &str = include_str!("script_runner.py");
/// how long Python may take to start and load the script
const LOAD_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize)]
pub struct ScriptInfo {
    pub python: String,
    pub numpy: Option<String>,
}

/// A trigger the script found.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct Found {
    /// s
    pub t: f64,
    pub label: Option<String>,
    pub width: Option<f64>,
    pub peak: Option<f64>,
    pub mean: Option<f64>,
    pub integral: Option<f64>,
}

impl Found {
    pub fn event(self) -> nbp_core::trigger::Event {
        let n = |x: Option<f64>| x.unwrap_or(f64::NAN);
        nbp_core::trigger::Event { seq: 0, t: self.t, what: 'c', width: n(self.width), peak: n(self.peak), mean: n(self.mean), integral: n(self.integral), from: f64::NAN, label: self.label }
    }
}

/// What the script said about a block.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Scan {
    pub events: Vec<Found>,
    /// what it printed
    pub out: String,
    /// the traceback, if it raised
    pub error: Option<String>,
    /// how long it took, ms
    pub ms: f64,
}

struct Proc {
    key: (String, String),
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    stderr: Arc<Mutex<String>>,
    info: ScriptInfo,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub struct ScriptHost {
    proc: Option<Proc>,
}

/// the interpreter to run: the one given, else python3 (python on Windows)
fn interpreter(python: &str) -> String {
    let p = python.trim();
    if !p.is_empty() {
        p.to_string()
    } else if cfg!(windows) {
        "python".into()
    } else {
        "python3".into()
    }
}

impl ScriptHost {
    /// Starts the script (if it isn't running already): the Python version, or why it can't.
    pub fn load(&mut self, python: &str, code: &str) -> Result<ScriptInfo, String> {
        let key = (interpreter(python), code.to_string());
        let alive = self.proc.as_mut().is_some_and(|p| p.key == key && matches!(p.child.try_wait(), Ok(None)));
        if !alive {
            self.proc = None;
            self.proc = Some(start(key)?);
        }
        Ok(self.proc.as_ref().unwrap().info.clone())
    }

    /// One block of samples (nbp-core `trigger::encode_block`). A script that takes longer than `timeout` is stopped.
    pub fn scan(&mut self, python: &str, code: &str, block: &[u8], timeout: Duration) -> Result<Scan, String> {
        let t0 = Instant::now();
        let head = serde_json::json!({ "block": block.len() }).to_string() + "\n";
        let v = self.ask(python, code, head.as_bytes(), block, timeout)?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        Ok(Scan {
            events: v.get("events").cloned().map(serde_json::from_value).transpose().map_err(|e| format!("the runner's events: {e}"))?.unwrap_or_default(),
            out: s("out").unwrap_or_default(),
            error: s("error"),
            ms: t0.elapsed().as_secs_f64() * 1e3,
        })
    }

    /// The trigger starts over: the script's reset(), if it has one (and it's running).
    pub fn reset(&mut self, python: &str, code: &str) -> Result<(), String> {
        if self.proc.as_ref().is_some_and(|p| p.key == (interpreter(python), code.to_string())) {
            self.ask(python, code, b"{\"reset\": true}\n", &[], Duration::from_secs(5))?;
        }
        Ok(())
    }

    /// a request (a line and its bytes), the answer's JSON
    fn ask(&mut self, python: &str, code: &str, line: &[u8], bytes: &[u8], timeout: Duration) -> Result<serde_json::Value, String> {
        self.load(python, code)?;
        let p = self.proc.as_mut().unwrap();
        if p.stdin.write_all(line).and_then(|_| p.stdin.write_all(bytes)).and_then(|_| p.stdin.flush()).is_err() {
            let why = died(p);
            self.proc = None;
            return Err(why);
        }
        let answer = match p.lines.recv_timeout(timeout) {
            Ok(a) => a,
            Err(RecvTimeoutError::Timeout) => {
                self.proc = None;
                return Err(format!("the script took longer than {} s on one block of samples: stopped", timeout.as_secs_f64()));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let why = died(p);
                self.proc = None;
                return Err(why);
            }
        };
        serde_json::from_str(&answer).map_err(|e| format!("the runner said something odd ({e}): {answer}"))
    }

    pub fn stop(&mut self) {
        self.proc = None;
    }
}

/// why the process went away: its exit and the end of what it wrote to stderr
fn died(p: &mut Proc) -> String {
    std::thread::sleep(Duration::from_millis(50));
    let code = p.child.try_wait().ok().flatten().map(|s| format!(" ({s})")).unwrap_or_default();
    let err = p.stderr.lock().unwrap().trim().to_string();
    if err.is_empty() {
        format!("Python stopped{code}")
    } else {
        format!("Python stopped{code}:\n{err}")
    }
}

fn start(key: (String, String)) -> Result<Proc, String> {
    let mut cmd = Command::new(&key.0);
    cmd.args(["-u", "-c", RUNNER])
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound || e.kind() == std::io::ErrorKind::PermissionDenied {
            format!("Python not found ({}): install Python 3, or give the interpreter's path", key.0)
        } else {
            format!("can't start {}: {e}", key.0)
        }
    })?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr_pipe = child.stderr.take().ok_or("no stderr")?;
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines() {
            let Ok(l) = l else { break };
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    // the end of stderr: tracebacks the runner didn't catch, the script's own warnings
    let stderr = Arc::new(Mutex::new(String::new()));
    let tail = stderr.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr_pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut s = tail.lock().unwrap();
            s.push_str(&String::from_utf8_lossy(&buf[..n]));
            if s.len() > 8000 {
                let cut = s.len() - 4000;
                let cut = (cut..s.len()).find(|&i| s.is_char_boundary(i)).unwrap_or(0);
                s.drain(..cut);
            }
        }
    });
    let mut p = Proc { key, child, stdin, lines, stderr, info: ScriptInfo { python: String::new(), numpy: None } };
    let init = serde_json::json!({ "code": p.key.1 }).to_string() + "\n";
    if p.stdin.write_all(init.as_bytes()).and_then(|_| p.stdin.flush()).is_err() {
        return Err(died(&mut p));
    }
    let answer = match p.lines.recv_timeout(LOAD_TIMEOUT) {
        Ok(a) => a,
        Err(RecvTimeoutError::Timeout) => return Err("Python didn't start in time (the script's top level may be slow)".into()),
        Err(RecvTimeoutError::Disconnected) => return Err(died(&mut p)),
    };
    let v: serde_json::Value = serde_json::from_str(&answer).map_err(|_| format!("not a Python 3 the runner understands: {answer}"))?;
    if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
        return Err(v.get("error").and_then(|x| x.as_str()).unwrap_or("the script didn't load").to_string());
    }
    p.info = ScriptInfo {
        python: v.get("python").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
        numpy: v.get("numpy").and_then(|x| x.as_str()).map(str::to_string),
    };
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_python() -> bool {
        Command::new(interpreter("")).arg("-V").output().is_ok()
    }

    fn block(t0: f64, i: &[f32]) -> Vec<u8> {
        nbp_core::trigger::encode_block(0.5, &[(t0, i.len() as u32)], i, &vec![3.0; i.len()], None)
    }

    #[test]
    fn finds_triggers_in_blocks() {
        if !have_python() {
            return;
        }
        // rising through 1.0, across blocks: the last sample kept in a global
        let code = "last = None\ndef reset():\n    global last\n    last = None\ndef trigger(data):\n    global last\n    found = []\n    for k in range(len(data)):\n        x = data.i[k]\n        if last is not None and last < 1.0 <= x:\n            found.append((data.t[k], f'{x:.1f} A'))\n        last = x\n    print(len(data))\n    return found\n";
        let mut h = ScriptHost::default();
        assert!(h.load("", code).unwrap().python.starts_with('3'));
        let a = h.scan("", code, &block(0.0, &[0.0, 2.0, 0.0, 0.0]), Duration::from_secs(5)).unwrap();
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].t, 0.5);
        assert_eq!(a.events[0].label.as_deref(), Some("2.0 A"));
        assert_eq!(a.out.trim(), "4");
        // the edge between the blocks
        let b = h.scan("", code, &block(2.0, &[3.0, 3.0]), Duration::from_secs(5)).unwrap();
        assert_eq!(b.events.iter().map(|e| e.t).collect::<Vec<_>>(), vec![2.0]);
        h.reset("", code).unwrap();
        let c = h.scan("", code, &block(3.0, &[3.0]), Duration::from_secs(5)).unwrap();
        assert!(c.events.is_empty());
        let e = a.events[0].clone().event();
        assert_eq!(e.what, 'c');
        assert!(e.width.is_nan());
    }

    #[test]
    fn says_what_is_wrong() {
        if !have_python() {
            return;
        }
        let mut h = ScriptHost::default();
        assert!(h.load("", "x = 1").unwrap_err().contains("trigger(data)"));
        assert!(h.load("", "def trigger(d):\n  return (").unwrap_err().contains("SyntaxError"));
        let code = "def trigger(d):\n    return 1 / 0\n";
        let v = h.scan("", code, &block(0.0, &[1.0]), Duration::from_secs(5)).unwrap();
        assert!(v.error.unwrap().contains("ZeroDivisionError"));
        let slow = "import time\ndef trigger(d):\n    time.sleep(5)\n";
        let err = h.scan("", slow, &block(0.0, &[1.0]), Duration::from_millis(300)).unwrap_err();
        assert!(err.contains("longer than"));
        assert!(h.load("/no/such/python", "def trigger(d): pass").unwrap_err().contains("not found"));
    }
}

/// where a block's samples end (s, just past the last one) and how many it has
pub fn block_end(b: &[u8]) -> Option<(f64, usize)> {
    let u = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()));
    let f = |o: usize| b.get(o..o + 8).map(|x| f64::from_le_bytes(x.try_into().unwrap()));
    let (n, runs, dt) = (u(0)? as usize, u(4)? as usize, f(12)?);
    if runs == 0 {
        return Some((f64::NAN, n));
    }
    let o = 20 + (runs - 1) * 12;
    Some((f(o)? + (u(o + 8)? as f64 - 0.5) * dt, n))
}

/// base64 (standard alphabet, padding optional) to bytes
pub fn unbase64(s: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for &c in s.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | val(c).ok_or("not base64")? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod example_tests {
    use super::*;

    /// an example script on samples (dt s apart, from 0), fed in uneven blocks: what it found
    fn run(code: &str, dt: f64, i: &[f32], v: &[f32]) -> Vec<Found> {
        let mut h = ScriptHost::default();
        let mut found = Vec::new();
        let mut a = 0;
        for (k, len) in [7777usize, 1, 3001, 12345].iter().cycle().enumerate() {
            if a >= i.len() || k > 100_000 {
                break;
            }
            let b = (a + len).min(i.len());
            let block = nbp_core::trigger::encode_block(dt, &[(a as f64 * dt, (b - a) as u32)], &i[a..b], &v[a..b], None);
            let s = h.scan("", code, &block, Duration::from_secs(20)).unwrap();
            assert!(s.error.is_none(), "{}", s.error.unwrap());
            found.extend(s.events);
            a = b;
        }
        found
    }

    fn have_numpy() -> bool {
        Command::new(interpreter("")).args(["-c", "import numpy"]).output().is_ok_and(|o| o.status.success())
    }

    macro_rules! example {
        ($f:literal) => {
            include_str!(concat!("../../ui/src/lib/script-examples/", $f))
        };
    }

    /// n samples of `x`
    fn flat(n: usize, x: f32) -> Vec<f32> {
        vec![x; n]
    }

    #[test]
    fn examples_find_what_they_say() {
        if !have_numpy() {
            return;
        }
        let dt = 1e-5;
        // edge: 5 pulses to 30 mA; between them it wobbles around HIGH without going below LOW
        let mut i = Vec::new();
        for _ in 0..5 {
            i.extend(flat(500, 0.0));
            i.extend(flat(300, 0.030));
            for k in 0..200 {
                i.push(if k % 2 == 0 { 0.019 } else { 0.021 });
            }
        }
        let v = flat(i.len(), 3.3);
        let e = run(example!("01-edge.py"), dt, &i, &v);
        assert_eq!(e.len(), 5, "edge {e:?}");

        // charge: 3 bursts of 20 mA for 2 ms (40 µC), 3 of 0.5 ms (10 µC)
        let mut i = Vec::new();
        for w in [200, 50, 200, 50, 200, 50] {
            i.extend(flat(1000, 0.0));
            i.extend(flat(w, 0.020));
        }
        i.extend(flat(100, 0.0));
        let v = flat(i.len(), 3.3);
        let e = run(example!("02-charge.py"), dt, &i, &v);
        assert_eq!(e.len(), 3, "charge {e:?}");
        assert!(e[0].label.as_deref().unwrap().starts_with("40.0 µC"), "{e:?}");

        // events: groups of 3 bursts 1.5 ms apart (10 mA, 0.4 ms: 4 µC each), every 100 ms
        let mut i = Vec::new();
        for _ in 0..4 {
            for _ in 0..3 {
                i.extend(flat(40, 0.010));
                i.extend(flat(110, 0.0));
            }
            i.extend(flat(10000 - 450, 0.0));
        }
        let v = flat(i.len(), 3.3);
        let e = run(example!("03-events.py"), dt, &i, &v);
        let labels: Vec<_> = e.iter().map(|e| e.label.clone().unwrap()).collect();
        assert_eq!(labels, vec!["3 bursts, 12.0 µC"; 4], "events {labels:?}");

        // brown-out: dips to 2.8 V for 100 µs (twice) and 20 µs (twice)
        let mut v = Vec::new();
        for w in [10, 2, 10, 2] {
            v.extend(flat(3000, 3.3));
            v.extend(flat(w, 2.8));
        }
        v.extend(flat(100, 3.3));
        let i = flat(v.len(), 0.001);
        let e = run(example!("04-brownout.py"), dt, &i, &v);
        assert_eq!(e.len(), 2, "brownout {e:?}");

        // heartbeat: a 1 ms burst every 100 ms; one 250 ms late; then none for 400 ms
        let dt4 = 1e-4;
        let mut i = Vec::new();
        for gap in [1000, 1000, 1000, 1000, 1000, 2500, 1000, 1000, 4000] {
            i.extend(flat(10, 0.020));
            i.extend(flat(gap - 10, 0.0));
        }
        let v = flat(i.len(), 3.3);
        let e = run(example!("05-heartbeat.py"), dt4, &i, &v);
        let labels: Vec<_> = e.iter().map(|e| e.label.clone().unwrap()).collect();
        assert_eq!(labels.len(), 2, "heartbeat {labels:?}");
        assert!(labels[0].starts_with("late") && labels[1] == "missed", "{labels:?}");

        // spikes: noise around 5 mA, three single-sample spikes of +5 mA
        let mut i: Vec<f32> = (0..30000).map(|k| 0.005 + 1e-4 * (((k * 7919) % 101) as f32 / 50.0 - 1.0)).collect();
        for k in [5000, 15000, 25000] {
            i[k] += 0.005;
        }
        let v = flat(i.len(), 3.3);
        let e = run(example!("06-spike.py"), dt, &i, &v);
        assert_eq!(e.len(), 3, "spike {e:?}");

        // dI/dt: two slow ramps to 30 mA (3 A/s), two steps in 100 µs (300 A/s); all down slowly
        let mut i = Vec::new();
        for up in [1000, 10, 1000, 10] {
            i.extend(flat(500, 0.0));
            i.extend((0..up).map(|k| 0.030 * k as f32 / up as f32));
            i.extend(flat(200, 0.030));
            i.extend((0..1000).map(|k| 0.030 * (1.0 - k as f32 / 1000.0)));
        }
        let v = flat(i.len(), 3.3);
        let e = run(example!("07-slew.py"), dt, &i, &v);
        assert_eq!(e.len(), 2, "slew {e:?}");

        // ripple: 10 kHz for 30 ms, 200 kHz for 30 ms, 10 kHz again
        let dt6 = 1e-6;
        let v: Vec<f32> = (0..90_000)
            .map(|k| {
                let t = k as f64 * dt6;
                let f = if (30_000..60_000).contains(&k) { 200e3 } else { 10e3 };
                (3.3 + 0.005 * (2.0 * std::f64::consts::PI * f * t).sin()) as f32
            })
            .collect();
        let i = flat(v.len(), 0.001);
        let e = run(example!("08-ripple.py"), dt6, &i, &v);
        let labels: Vec<_> = e.iter().map(|e| e.label.clone().unwrap()).collect();
        assert_eq!(labels.len(), 2, "ripple {labels:?}");
        assert!(labels[0].contains("(out)") && !labels[1].contains("(out)"), "{labels:?}");
    }
}

#[cfg(test)]
mod block_tests {
    #[test]
    fn base64_and_block_end() {
        assert_eq!(super::unbase64("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        let b = nbp_core::trigger::encode_block(0.5, &[(0.0, 2), (10.0, 4)], &[0.0; 6], &[0.0; 6], None);
        assert_eq!(super::block_end(&b), Some((11.75, 6)));
    }
}

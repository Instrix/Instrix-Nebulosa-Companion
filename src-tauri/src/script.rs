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
mod block_tests {
    #[test]
    fn base64_and_block_end() {
        assert_eq!(super::unbase64("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        let b = nbp_core::trigger::encode_block(0.5, &[(0.0, 2), (10.0, 4)], &[0.0; 6], &[0.0; 6], None);
        assert_eq!(super::block_end(&b), Some((11.75, 6)));
    }
}

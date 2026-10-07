//! The plugin host process: `calibre-debug -e host.py`, talking JSON lines.
//!
//! One process serves every request (plugins keep state between calls, such
//! as the cover URLs a metadata source finds while identifying); requests
//! run concurrently in it and replies are matched by id. What the host and
//! plugins print is kept as a log.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

/// The host script, written next to the plugins at start-up.
pub const SCRIPT: &str = include_str!("host.py");

/// Lines of the host's log kept (stderr: calibre's and plugins' output, tracebacks).
const LOG_LINES: usize = 400;

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, String>>>>>;

pub struct Host {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next: AtomicU64,
    alive: Arc<AtomicBool>,
    log: Arc<Mutex<VecDeque<String>>>,
}

/// `calibre-debug` beside calibre's `ebook-convert`.
pub fn calibre_debug_beside(ebook_convert: &Path) -> PathBuf {
    let name = if cfg!(windows) { "calibre-debug.exe" } else { "calibre-debug" };
    ebook_convert.with_file_name(name)
}

impl Host {
    /// Start the host with `calibre_debug`, running `script`, with plugin
    /// settings kept in `config_dir` (never calibre's own).
    pub fn start(calibre_debug: &Path, script: &Path, config_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(calibre_debug);
        cmd.arg("-e")
            .arg(script)
            .env("CALIBRE_CONFIG_DIRECTORY", config_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child =
            cmd.spawn().map_err(|e| format!("couldn't start calibre ({}): {e}", calibre_debug.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;

        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let log: Arc<Mutex<VecDeque<String>>> = Arc::default();

        let (p, a) = (pending.clone(), alive.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(reply) = serde_json::from_str::<Value>(&line) else { continue };
                let Some(id) = reply.get("id").and_then(Value::as_u64) else { continue };
                let answer = match reply.get("error").and_then(Value::as_str) {
                    Some(e) => Err(e.to_owned()),
                    None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
                };
                if let Some(tx) = p.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                    let _ = tx.send(answer);
                }
            }
            // The host is gone: nothing more will be answered.
            a.store(false, Ordering::SeqCst);
            for (_, tx) in p.lock().unwrap_or_else(|e| e.into_inner()).drain() {
                let _ = tx.send(Err("the plugin host stopped".into()));
            }
        });
        let l = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                let mut log = l.lock().unwrap_or_else(|e| e.into_inner());
                if log.len() == LOG_LINES {
                    log.pop_front();
                }
                log.push_back(line);
            }
        });

        Ok(Self { child: Mutex::new(child), stdin: Mutex::new(stdin), pending, next: AtomicU64::new(1), alive, log })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Call `method`, waiting at most `timeout` for the reply.
    pub fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        if !self.is_alive() {
            return Err("the plugin host stopped".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        let line = json!({ "id": id, "method": method, "params": params }).to_string();
        let sent = {
            let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
            writeln!(stdin, "{line}").and_then(|()| stdin.flush())
        };
        if let Err(e) = sent {
            self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
            return Err(format!("the plugin host stopped: {e}"));
        }
        match rx.recv_timeout(timeout) {
            Ok(answer) => answer,
            Err(_) => {
                self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                Err(format!("no answer within {} seconds", timeout.as_secs()))
            }
        }
    }

    /// The host's recent output, oldest first.
    pub fn log(&self) -> Vec<String> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Ok(mut stdin) = self.stdin.lock() {
            let _ = writeln!(stdin, r#"{{"id":0,"method":"shutdown"}}"#);
        }
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        // A moment to exit cleanly, then make sure.
        for _ in 0..20 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

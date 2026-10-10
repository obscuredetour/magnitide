//! The plugin host process: a calibre's Python or Estuary's running
//! `magnitide_host` (from the compat package), talking JSON lines.
//!
//! One process serves every request (plugins keep state between calls, such
//! as the cover URLs a metadata source finds while identifying); requests
//! run concurrently in it and replies are matched by id. Events the host
//! sends unasked (a GUI plugin's status message, error, or that it's done)
//! go to an [`EventHandler`]. What the host and plugins print is kept as a log.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::estuary::Estuary;

/// Lines of the host's log kept (stderr: calibre's and plugins' output, tracebacks).
const LOG_LINES: usize = 400;

/// Starts the host package from the compat directory given as the first
/// argument. The directory goes on `sys.path` this way, not through
/// PYTHONPATH, because the interpreter runs with `-I` (isolated: the user's
/// PYTHON* variables and site-packages are ignored).
/// Run in Estuary: `<python dir>/host` and `<python dir>/compat`
/// on the path (the host package, then our `calibre`), then the host.
const BOOTSTRAP: &str = "import sys, runpy, os; d = sys.argv.pop(1); \
sys.path[0:0] = [os.path.join(d, 'host'), os.path.join(d, 'compat')]; \
runpy.run_module('magnitide_host', run_name='__main__', alter_sys=True)";

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, String>>>>>;

/// Called (on the host's reader thread) with each event: `{"event": "gui-idle", …}`.
pub type EventHandler = Arc<dyn Fn(Value) + Send + Sync>;

/// How the crate talks to a host: the real process, or a stand-in in tests.
pub trait Transport: Send + Sync {
    fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String>;
    fn is_alive(&self) -> bool;
    fn log(&self) -> Vec<String>;
}

pub struct Host {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next: AtomicU64,
    alive: Arc<AtomicBool>,
    log: Arc<Mutex<VecDeque<String>>>,
}

/// Run inside an installed calibre (`calibre-debug -c`): only the host
/// package goes on the path; `calibre`, `qt` and the rest are calibre's.
const BOOTSTRAP_CALIBRE: &str = "import sys, runpy, os; d = sys.argv.pop(1); \
sys.path.insert(0, os.path.join(d, 'host')); \
runpy.run_module('magnitide_host', run_name='__main__', alter_sys=True)";

impl Host {
    /// Start the host in `estuary`, with the compat package at `compat_dir`
    /// and plugin settings kept in `config_dir` (never calibre's own).
    pub fn start(
        estuary: &Estuary,
        compat_dir: &Path,
        config_dir: &Path,
        on_event: Option<EventHandler>,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(&estuary.python);
        cmd.args(["-I", "-X", "utf8", "-u", "-c", BOOTSTRAP])
            .arg(compat_dir)
            .env("MAGNITIDE_HOST_MODE", "estuary")
            // A system Qt must not leak into Estuary's.
            .env_remove("QT_PLUGIN_PATH")
            .env_remove("QT_QPA_PLATFORMTHEME");
        Self::spawn(cmd, config_dir, on_event, &format!("Estuary ({})", estuary.python.display()))
    }

    /// Start the host inside an installed calibre, through its
    /// `calibre-debug`: plugins run on calibre's own code, with the host
    /// package from `compat_dir`. Settings stay in `config_dir`, never in
    /// the user's calibre configuration.
    pub fn start_calibre(
        calibre_debug: &Path,
        compat_dir: &Path,
        config_dir: &Path,
        on_event: Option<EventHandler>,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(calibre_debug);
        cmd.args(["-c", BOOTSTRAP_CALIBRE]).arg(compat_dir).env("MAGNITIDE_HOST_MODE", "calibre");
        Self::spawn(cmd, config_dir, on_event, &format!("calibre ({})", calibre_debug.display()))
    }

    fn spawn(mut cmd: Command, config_dir: &Path, on_event: Option<EventHandler>, what: &str) -> Result<Self, String> {
        std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
        let cache_dir = config_dir.join("caches");
        cmd.env("CALIBRE_CONFIG_DIRECTORY", config_dir)
            .env("CALIBRE_CACHE_DIRECTORY", &cache_dir)
            // So the host can give the focus back when a plugin's windows close.
            .env("MAGNITIDE_PID", std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("couldn't start {what}: {e}"))?;
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
                if reply.get("event").is_some() {
                    if let Some(f) = &on_event {
                        f(reply);
                    }
                    continue;
                }
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

impl Transport for Host {
    fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        Host::call(self, method, params, timeout)
    }

    fn is_alive(&self) -> bool {
        Host::is_alive(self)
    }

    fn log(&self) -> Vec<String> {
        Host::log(self)
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

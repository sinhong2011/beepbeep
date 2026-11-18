// SPDX-License-Identifier: AGPL-3.0-or-later
//! One queue per user (plan §21, §27): the first BeepBeep process to start — the desktop app, the CLI
//! or `beep mcp` — owns the download service and serves it on a local socket; every later one attaches
//! as a client. So downloads an AI assistant adds appear live in the window, and only one process ever
//! writes the queue's files.
//!
//! On macOS and Linux it is a Unix socket, `$TMPDIR/beepbeep-<hash of the data folder>.sock`, mode
//! 0600 in the user's private temporary folder. On Windows it is a named pipe,
//! `\\.\pipe\beepbeep-<hash>`, that refuses remote clients. Either way: local only, no network port. The protocol is one JSON request per line,
//! `{"method": "...", "params": {...}}`, answered by one line `{"ok": ...}` or `{"error": "..."}`.

use crate::domain::DownloadTask;
use crate::manager::{Event, Manager, Snapshot, Store};
use crate::settings::{Quality, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A task on the wire, with the runtime metrics the store does not persist.
#[derive(Serialize, Deserialize)]
struct WireTask {
    #[serde(flatten)]
    task: DownloadTask,
    speed: f64,
    connections: usize,
}

#[derive(Serialize, Deserialize)]
struct WireSnapshot {
    version: u64,
    tasks: Vec<WireTask>,
    settings: Settings,
}

impl From<Snapshot> for WireSnapshot {
    fn from(s: Snapshot) -> Self {
        WireSnapshot {
            version: s.version,
            tasks: s.tasks.into_iter().map(|t| WireTask { speed: t.speed, connections: t.connections, task: t }).collect(),
            settings: s.settings,
        }
    }
}

impl From<WireSnapshot> for Snapshot {
    fn from(w: WireSnapshot) -> Self {
        Snapshot {
            version: w.version,
            tasks: w
                .tasks
                .into_iter()
                .map(|mut t| {
                    t.task.speed = t.speed;
                    t.task.connections = t.connections;
                    t.task
                })
                .collect(),
            settings: w.settings,
        }
    }
}

/// Who holds the queue in this process.
pub enum Role {
    /// This process owns the queue and serves it to others.
    Owner,
    /// Another BeepBeep process owns it; calls go over its socket.
    Client,
}

/// The download service, owned here or attached to the owner. Same calls either way. An attached
/// service takes the queue over when its owner quits.
pub struct Service {
    dir: Option<PathBuf>,
    inner: std::sync::RwLock<Inner>,
}

enum Inner {
    Local(Manager),
    #[cfg(any(unix, windows))]
    Remote(client::Client),
}

/// The control socket of `dir`'s queue. It lives in the per-user temporary folder (private to the
/// user on macOS and Linux) under a short name derived from `dir`: socket paths are limited to about
/// 104 bytes, which a data folder deep in the file system can exceed.
pub fn socket_path(dir: &Path) -> PathBuf {
    let name = endpoint_name(dir);
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir().join(format!("{name}.sock"))
    }
}

fn endpoint_name(dir: &Path) -> String {
    // FNV-1a: stable across builds and platforms, unlike `DefaultHasher`.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in dir.as_os_str().as_encoded_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
    }
    format!("beepbeep-{:012x}", h & 0xffff_ffff_ffff)
}

fn own(dir: &Path, settings: Option<Settings>) -> Manager {
    let m = Manager::new(Store::at(dir.to_owned()), settings);
    #[cfg(any(unix, windows))]
    server::serve(&m, &socket_path(dir));
    m
}

impl Service {
    /// Attaches to the queue's owner when one is running, else becomes the owner of `dir`'s queue.
    pub fn connect_or_start(dir: PathBuf, settings: Option<Settings>) -> Service {
        if let Some(s) = Service::connect(&dir) {
            return s;
        }
        let m = own(&dir, settings);
        Service { dir: Some(dir), inner: std::sync::RwLock::new(Inner::Local(m)) }
    }

    /// Attaches to a running owner of `dir`'s queue; `None` when no BeepBeep process owns it.
    pub fn connect(dir: &Path) -> Option<Service> {
        #[cfg(any(unix, windows))]
        {
            let c = client::Client::connect(&socket_path(dir))?;
            Some(Service { dir: Some(dir.to_owned()), inner: std::sync::RwLock::new(Inner::Remote(c)) })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = dir;
            None
        }
    }

    /// A service only this process uses (tests, `--state DIR`).
    pub fn private(m: Manager) -> Service {
        Service { dir: None, inner: std::sync::RwLock::new(Inner::Local(m)) }
    }

    pub fn role(&self) -> Role {
        match &*self.inner.read().unwrap() {
            Inner::Local(_) => Role::Owner,
            #[cfg(any(unix, windows))]
            Inner::Remote(_) => Role::Client,
        }
    }

    /// The owner went away: attach to whoever owns the queue now, or own it here.
    #[cfg(any(unix, windows))]
    fn take_over(&self) {
        let Some(dir) = &self.dir else { return };
        let mut inner = self.inner.write().unwrap();
        if let Inner::Remote(c) = &*inner {
            if c.call("snapshot", json!({ "since": u64::MAX })).is_some() {
                return; // it recovered (or another process took over first)
            }
            *inner = match client::Client::connect(&socket_path(dir)) {
                Some(c) => Inner::Remote(c),
                None => Inner::Local(own(dir, None)),
            };
            log::info!("the queue's owner quit; this process took over");
        }
    }

    fn with_local<R>(&self, f: impl FnOnce(&Manager) -> R) -> Option<R> {
        match &*self.inner.read().unwrap() {
            Inner::Local(m) => Some(f(m)),
            #[cfg(any(unix, windows))]
            Inner::Remote(_) => None,
        }
    }

    fn call(&self, method: &str, params: Value) -> Value {
        for _ in 0..2 {
            let answer = match &*self.inner.read().unwrap() {
                Inner::Local(m) => return dispatch(m, method, &params).unwrap_or(Value::Null),
                #[cfg(any(unix, windows))]
                Inner::Remote(c) => c.call(method, params.clone()),
            };
            match answer {
                Some(v) => return v,
                #[cfg(any(unix, windows))]
                None => self.take_over(),
                #[cfg(not(any(unix, windows)))]
                None => return Value::Null,
            }
        }
        Value::Null
    }

    pub fn snapshot(&self) -> Snapshot {
        self.with_local(|m| m.snapshot()).unwrap_or_else(|| {
            self.snapshot_since(u64::MAX).unwrap_or_else(|| Snapshot { version: 0, tasks: vec![], settings: Settings::default() })
        })
    }

    pub fn snapshot_since(&self, version: u64) -> Option<Snapshot> {
        if let Some(s) = self.with_local(|m| m.snapshot_since(version)) {
            return s;
        }
        let v = self.call("snapshot", json!({ "since": version }));
        serde_json::from_value::<Option<WireSnapshot>>(v).ok().flatten().map(Snapshot::from)
    }

    pub fn take_events(&self) -> Vec<Event> {
        self.with_local(|m| m.take_events())
            .unwrap_or_else(|| serde_json::from_value(self.call("take_events", json!({}))).unwrap_or_default())
    }

    pub fn settings(&self) -> Settings {
        self.with_local(|m| m.settings()).unwrap_or_else(|| self.snapshot().settings)
    }

    /// Changes one setting by name (`Settings::set`).
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        match self.call("set_setting", json!({ "key": key, "value": value })) {
            Value::String(e) => Err(e),
            _ => Ok(()),
        }
    }

    pub fn add_urls(&self, urls: &[String], quality: Option<Quality>) -> Vec<Uuid> {
        serde_json::from_value(self.call("add_urls", json!({ "urls": urls, "quality": quality }))).unwrap_or_default()
    }

    pub fn add_text(&self, text: &str, quality: Option<Quality>) -> Vec<Uuid> {
        self.add_urls(&crate::util::extract_links(text), quality)
    }

    pub fn pause(&self, id: Uuid) {
        self.call("pause", json!({ "id": id }));
    }
    pub fn resume(&self, id: Uuid) {
        self.call("resume", json!({ "id": id }));
    }
    pub fn stop(&self, id: Uuid) {
        self.call("stop", json!({ "id": id }));
    }
    pub fn redownload(&self, id: Uuid) {
        self.call("redownload", json!({ "id": id }));
    }
    pub fn remove(&self, id: Uuid, delete_file: bool) -> Option<PathBuf> {
        serde_json::from_value(self.call("remove", json!({ "id": id, "delete_file": delete_file }))).ok().flatten()
    }
    pub fn pause_all(&self) {
        self.call("pause_all", json!({}));
    }
    pub fn resume_all(&self) {
        self.call("resume_all", json!({}));
    }
    pub fn clear_finished(&self) {
        self.call("clear_finished", json!({}));
    }
    pub fn flush(&self) {
        self.call("flush", json!({}));
    }
}

/// Runs one request against the owner's manager. `Some(value)` is the answer.
fn dispatch(m: &Manager, method: &str, p: &Value) -> Option<Value> {
    let id = || p["id"].as_str().and_then(|s| Uuid::parse_str(s).ok());
    Some(match method {
        "snapshot" => json!(m.snapshot_since(p["since"].as_u64().unwrap_or(0)).map(WireSnapshot::from)),
        "take_events" => json!(m.take_events()),
        "set_setting" => {
            let (key, value) = (p["key"].as_str()?, p["value"].as_str()?);
            let mut err = None;
            m.update_settings(|s| err = s.set(key, value).err());
            json!(err)
        }
        "add_urls" => {
            let urls: Vec<String> = serde_json::from_value(p["urls"].clone()).ok()?;
            let quality: Option<Quality> = serde_json::from_value(p["quality"].clone()).ok().flatten();
            json!(m.add_urls(&urls, quality))
        }
        "pause" => {
            m.pause(id()?);
            Value::Null
        }
        "resume" => {
            m.resume(id()?);
            Value::Null
        }
        "stop" => {
            m.stop(id()?);
            Value::Null
        }
        "redownload" => {
            m.redownload(id()?);
            Value::Null
        }
        "remove" => json!(m.remove(id()?, p["delete_file"].as_bool().unwrap_or(false))),
        "pause_all" => {
            m.pause_all();
            Value::Null
        }
        "resume_all" => {
            m.resume_all();
            Value::Null
        }
        "clear_finished" => {
            m.clear_finished();
            Value::Null
        }
        "flush" => {
            m.flush();
            Value::Null
        }
        _ => return None,
    })
}

#[cfg(any(unix, windows))]
mod server {
    use super::dispatch;
    use crate::manager::Manager;
    use serde_json::{Value, json};
    use std::path::Path;
    use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

    /// Answers one connection's requests, line by line.
    async fn answer<S: AsyncRead + AsyncWrite + Unpin>(m: Manager, stream: S) {
        let (r, mut w) = tokio::io::split(stream);
        let mut lines = BufReader::new(r).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(req) => match dispatch(&m, req["method"].as_str().unwrap_or(""), &req["params"]) {
                    Some(v) => json!({ "ok": v }),
                    None => json!({ "error": "bad request" }),
                },
                Err(e) => json!({ "error": e.to_string() }),
            };
            if w.write_all(format!("{reply}\n").as_bytes()).await.is_err() {
                break;
            }
        }
    }

    /// Serves `m` on `endpoint` from the manager's own runtime.
    #[cfg(unix)]
    pub(super) fn serve(m: &Manager, sock: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::remove_file(sock); // a stale socket from a crash: nobody answered on it
        let listener = match std::os::unix::net::UnixListener::bind(sock) {
            Ok(l) => l,
            Err(e) => {
                log::warn!("control socket unavailable ({e}); this queue stays private to this process");
                return;
            }
        };
        let _ = std::fs::set_permissions(sock, std::fs::Permissions::from_mode(0o600));
        let _ = listener.set_nonblocking(true);
        let m = m.clone();
        m.clone().spawn(async move {
            let Ok(listener) = tokio::net::UnixListener::from_std(listener) else { return };
            loop {
                let Ok((stream, _)) = listener.accept().await else { continue };
                tokio::spawn(answer(m.clone(), stream));
            }
        });
    }

    /// Serves `m` on a named pipe: one pipe instance per client, the next made before the last is handed off.
    #[cfg(windows)]
    pub(super) fn serve(m: &Manager, pipe: &Path) {
        use tokio::net::windows::named_pipe::ServerOptions;
        let name = pipe.as_os_str().to_owned();
        let m = m.clone();
        m.clone().spawn(async move {
            let mut first = true;
            loop {
                let server = match ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).create(&name) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("control pipe unavailable ({e}); this queue stays private to this process");
                        return;
                    }
                };
                first = false;
                if server.connect().await.is_ok() {
                    tokio::spawn(answer(m.clone(), server));
                }
            }
        });
    }
}

#[cfg(any(unix, windows))]
mod client {
    use serde_json::{Value, json};
    use std::io::{BufRead, BufReader, Write};
    #[cfg(unix)]
    use std::os::unix::net::UnixStream as Stream;
    #[cfg(windows)]
    use std::fs::File as Stream;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    #[cfg(unix)]
    use std::time::Duration;

    pub(super) struct Client {
        path: PathBuf,
        conn: Mutex<Option<(Stream, BufReader<Stream>)>>,
    }

    fn open(path: &Path) -> Option<(Stream, BufReader<Stream>)> {
        #[cfg(unix)]
        let s = {
            let s = Stream::connect(path).ok()?;
            s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
            s
        };
        // A named pipe opens like a file; the owner answers each line it reads.
        #[cfg(windows)]
        let s = std::fs::OpenOptions::new().read(true).write(true).open(path).ok()?;
        let r = BufReader::new(s.try_clone().ok()?);
        Some((s, r))
    }

    impl Client {
        /// Connects when an owner answers a ping.
        pub(super) fn connect(path: &Path) -> Option<Client> {
            let c = Client { path: path.to_owned(), conn: Mutex::new(open(path)) };
            c.call("snapshot", json!({ "since": u64::MAX })).map(|_| c)
        }

        /// One request; reconnects once if the owner's connection dropped.
        pub(super) fn call(&self, method: &str, params: Value) -> Option<Value> {
            let req = format!("{}\n", json!({ "method": method, "params": params }));
            let mut guard = self.conn.lock().ok()?;
            for _ in 0..2 {
                if guard.is_none() {
                    *guard = open(&self.path);
                }
                let Some((w, r)) = guard.as_mut() else { return None };
                let mut line = String::new();
                if w.write_all(req.as_bytes()).is_ok() && r.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                    let v: Value = serde_json::from_str(&line).ok()?;
                    return v.get("ok").cloned();
                }
                *guard = None;
            }
            None
        }
    }
}

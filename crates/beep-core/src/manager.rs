// SPDX-License-Identifier: AGPL-3.0-or-later
//! The download service (plan §21, §23): a queue with bounded concurrency, lifecycle transitions,
//! crash recovery, coalesced progress (4 Hz, EMA speed) and atomic persistence.
//!
//! The manager owns its own Tokio runtime, so a UI (NSH), the CLI or a headless service drive it the
//! same way: call commands, then poll `snapshot_since(version)` at the UI's cadence.

use crate::domain::{DownloadTask, MediaInfo, Platform, TaskKind, TaskStatus};
use crate::engine::{self, EngineError, Job, Progress, RateLimiter};
use crate::providers::{MediaFormat, MediaItem, ResolveError, ResolverChain, YtDlpProgress, classify};
use crate::settings::{Quality, Settings};
use crate::util::{self, fmt_bytes, now_ms};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------------------------------------
// Store

/// Atomic JSON persistence (audit A6): written to a temp file, then renamed.
#[derive(Clone)]
pub struct Store {
    dir: Option<PathBuf>,
}

impl Store {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: Some(dir.into()) }
    }
    pub fn memory() -> Self {
        Self { dir: None }
    }
    pub fn load<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        let data = std::fs::read(self.dir.as_ref()?.join(name)).ok()?;
        serde_json::from_slice(&data).ok()
    }
    pub fn save<T: Serialize>(&self, name: &str, value: &T) {
        let Some(dir) = &self.dir else { return };
        let Ok(data) = serde_json::to_vec_pretty(value) else { return };
        let _ = std::fs::create_dir_all(dir);
        let tmp = dir.join(format!(".{name}.tmp"));
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(name));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Public API

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub enum Event {
    Finished(DownloadTask),
    Failed(DownloadTask),
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub version: u64,
    pub tasks: Vec<DownloadTask>,
    pub settings: Settings,
}

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

struct State {
    tasks: Vec<DownloadTask>,
    settings: Settings,
    version: u64,
    dirty: bool,
    events: Vec<Event>,
}

struct Running {
    token: u64,
    abort: tokio::task::AbortHandle,
    progress: Arc<Progress>,
    last_bytes: u64,
}

struct Inner {
    state: Mutex<State>,
    jobs: Mutex<HashMap<Uuid, Running>>,
    limiter: Arc<RateLimiter>,
    store: Store,
    rt: tokio::runtime::Runtime,
    next_token: std::sync::atomic::AtomicU64,
}

impl Manager {
    pub fn new(store: Store, settings: Option<Settings>) -> Self {
        let settings = settings.or_else(|| store.load("settings.json")).unwrap_or_default();
        let mut tasks: Vec<DownloadTask> = store.load("tasks.json").unwrap_or_default();
        // Crash recovery: anything mid-flight goes back to the queue and resumes from its segments.
        for t in tasks.iter_mut().filter(|t| t.status.is_active() || t.status == TaskStatus::Queued) {
            t.status = TaskStatus::Queued;
            t.push_log("Recovered after restart");
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("beep-engine")
            .enable_all()
            .build()
            .expect("tokio runtime");
        let inner = Arc::new(Inner {
            limiter: Arc::new(RateLimiter::new(settings.speed_limit)),
            state: Mutex::new(State { tasks, settings, version: 1, dirty: false, events: Vec::new() }),
            jobs: Mutex::new(HashMap::new()),
            store,
            rt,
            next_token: 1.into(),
        });
        let weak = Arc::downgrade(&inner);
        inner.rt.spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            let mut n = 0u64;
            loop {
                tick.tick().await;
                let Some(inner) = weak.upgrade() else { break };
                inner.sample();
                n += 1;
                if n % 8 == 0 {
                    inner.save_if_dirty();
                }
            }
        });
        let m = Manager { inner };
        m.inner.pump();
        m
    }

    /// Runs `f` on the service's runtime (the control socket lives there).
    pub fn spawn<F>(&self, f: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.inner.rt.spawn(f);
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.inner.state.lock().unwrap();
        Snapshot { version: s.version, tasks: s.tasks.clone(), settings: s.settings.clone() }
    }

    /// `None` when nothing changed since `version`; the cheap path for a UI timer.
    pub fn snapshot_since(&self, version: u64) -> Option<Snapshot> {
        let s = self.inner.state.lock().unwrap();
        (s.version != version).then(|| Snapshot { version: s.version, tasks: s.tasks.clone(), settings: s.settings.clone() })
    }

    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.inner.state.lock().unwrap().events)
    }

    pub fn settings(&self) -> Settings {
        self.inner.state.lock().unwrap().settings.clone()
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) {
        let (s, concurrency_changed) = {
            let mut st = self.inner.state.lock().unwrap();
            let before = st.settings.max_concurrent;
            f(&mut st.settings);
            st.version += 1;
            (st.settings.clone(), before != st.settings.max_concurrent)
        };
        self.inner.limiter.set_rate(s.speed_limit);
        self.inner.store.save("settings.json", &s);
        if concurrency_changed {
            self.inner.pump();
        }
    }

    /// Adds each URL as a task: post links of X, Threads and Instagram become media tasks.
    pub fn add_urls(&self, urls: &[String], quality: Option<Quality>) -> Vec<Uuid> {
        let mut ids = Vec::new();
        {
            let mut st = self.inner.state.lock().unwrap();
            let dir = st.settings.download_dir.clone();
            let conns = st.settings.connections_per_task;
            for u in urls {
                let media = classify(u).map(|p| {
                    let mut m = MediaInfo::new(p, u.as_str());
                    m.quality = quality;
                    m
                });
                let kind = if media.is_some() { TaskKind::Media } else { TaskKind::Http };
                let mut t = DownloadTask::new(kind, u, dir.clone(), conns, media);
                t.push_log(format!("Added {u}"));
                if st.tasks.iter().any(|o| o.source == *u && !o.status.is_terminal()) {
                    t.push_log("Note: this link is already in the list");
                }
                ids.push(t.id);
                // Newest first, but a batch keeps the order it was pasted in.
                let at = ids.len() - 1;
                st.tasks.insert(at, t);
            }
            st.version += 1;
            st.dirty = true;
        }
        self.inner.pump();
        ids
    }

    /// Link Collector: adds every http(s) link found in pasted text.
    pub fn add_text(&self, text: &str, quality: Option<Quality>) -> Vec<Uuid> {
        self.add_urls(&util::extract_links(text), quality)
    }

    pub fn pause(&self, id: Uuid) {
        let status = self.inner.with_task(id, |t| t.status);
        if status.is_none_or(|s| s.is_terminal() || s == TaskStatus::Paused) {
            return;
        }
        self.inner.stop_job(id);
        self.inner.update(id, |t| {
            t.status = TaskStatus::Paused;
            t.speed = 0.0;
            t.connections = 0;
            t.push_log("Paused");
        });
        self.inner.pump();
    }

    pub fn resume(&self, id: Uuid) {
        let ok = self
            .inner
            .with_task(id, |t| matches!(t.status, TaskStatus::Paused | TaskStatus::Failed | TaskStatus::Cancelled))
            .unwrap_or(false);
        if !ok {
            return;
        }
        self.inner.update(id, |t| {
            t.status = TaskStatus::Queued;
            t.error = None;
            t.push_log("Queued");
        });
        self.inner.pump();
    }

    /// Starts a finished task over (after the file was removed, or to pick up a new quality).
    pub fn redownload(&self, id: Uuid) {
        self.inner.stop_job(id);
        self.inner.update(id, |t| {
            if let Some(p) = t.part_path() {
                let _ = std::fs::remove_file(p);
            }
            t.status = TaskStatus::Queued;
            t.segments.clear();
            t.received_bytes = 0;
            t.total_bytes = None;
            t.final_path = None;
            t.file_name = None;
            t.completed_ms = None;
            t.error = None;
            t.sha256 = None;
            if t.kind == TaskKind::Media {
                t.fetch_url = None;
                if let Some(m) = &mut t.media {
                    m.ytdlp_format = None;
                }
            }
            t.push_log("Download again");
        });
        self.inner.pump();
    }

    pub fn stop(&self, id: Uuid) {
        self.inner.stop_job(id);
        self.inner.update(id, |t| {
            if !t.status.is_terminal() {
                t.status = TaskStatus::Cancelled;
                t.speed = 0.0;
                t.connections = 0;
                t.push_log("Stopped");
            }
        });
        self.inner.pump();
    }

    /// Removes a task and its partial file; `delete_file` also moves the finished file to the trash.
    pub fn remove(&self, id: Uuid, delete_file: bool) -> Option<PathBuf> {
        self.inner.stop_job(id);
        let removed = {
            let mut st = self.inner.state.lock().unwrap();
            let i = st.tasks.iter().position(|t| t.id == id)?;
            st.version += 1;
            st.dirty = true;
            st.tasks.remove(i)
        };
        if let Some(p) = removed.part_path() {
            let _ = std::fs::remove_file(p);
        }
        self.inner.pump();
        if delete_file { removed.final_path } else { None }
    }

    pub fn pause_all(&self) {
        let ids: Vec<Uuid> = self.ids_where(|t| t.status.is_active() || t.status == TaskStatus::Queued);
        ids.into_iter().for_each(|id| self.pause(id));
    }

    pub fn resume_all(&self) {
        let ids: Vec<Uuid> = self.ids_where(|t| t.status == TaskStatus::Paused);
        ids.into_iter().for_each(|id| self.resume(id));
    }

    pub fn clear_finished(&self) {
        let mut st = self.inner.state.lock().unwrap();
        st.tasks.retain(|t| !matches!(t.status, TaskStatus::Completed | TaskStatus::Cancelled));
        st.version += 1;
        st.dirty = true;
    }

    pub fn flush(&self) {
        self.inner.sample();
        let st = self.inner.state.lock().unwrap();
        self.inner.store.save("tasks.json", &st.tasks);
        self.inner.store.save("settings.json", &st.settings);
    }

    fn ids_where(&self, f: impl Fn(&DownloadTask) -> bool) -> Vec<Uuid> {
        self.inner.state.lock().unwrap().tasks.iter().filter(|t| f(t)).map(|t| t.id).collect()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let st = self.state.lock().unwrap();
        self.store.save("tasks.json", &st.tasks);
    }
}

// ---------------------------------------------------------------------------------------------
// Internals

enum Failure {
    Engine(EngineError),
    Resolve(ResolveError),
    Io(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Engine(e) => e.fmt(f),
            Failure::Resolve(e) => e.fmt(f),
            Failure::Io(s) => f.write_str(s),
        }
    }
}

impl From<EngineError> for Failure {
    fn from(e: EngineError) -> Self {
        Failure::Engine(e)
    }
}
impl From<ResolveError> for Failure {
    fn from(e: ResolveError) -> Self {
        Failure::Resolve(e)
    }
}
impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Failure::Io(e.to_string())
    }
}

impl Inner {
    fn with_task<R>(&self, id: Uuid, f: impl FnOnce(&DownloadTask) -> R) -> Option<R> {
        self.state.lock().unwrap().tasks.iter().find(|t| t.id == id).map(f)
    }

    fn task(&self, id: Uuid) -> Option<DownloadTask> {
        self.with_task(id, Clone::clone)
    }

    fn update(&self, id: Uuid, f: impl FnOnce(&mut DownloadTask)) {
        let mut st = self.state.lock().unwrap();
        if let Some(t) = st.tasks.iter_mut().find(|t| t.id == id) {
            f(t);
            t.updated_ms = now_ms();
            st.version += 1;
            st.dirty = true;
        }
    }

    fn settings(&self) -> Settings {
        self.state.lock().unwrap().settings.clone()
    }

    fn stop_job(&self, id: Uuid) {
        if let Some(job) = self.jobs.lock().unwrap().remove(&id) {
            job.abort.abort();
            // Keep the segment map consistent with the bytes already on disk.
            let segs = job.progress.segments.lock().unwrap().clone();
            if !segs.is_empty() {
                self.update(id, |t| {
                    t.received_bytes = segs.iter().map(|s| s.received.min(s.len())).sum();
                    t.segments = segs;
                });
            }
        }
    }

    fn pump(self: &Arc<Self>) {
        let (queued, max) = {
            let st = self.state.lock().unwrap();
            let mut q: Vec<(u64, Uuid)> =
                st.tasks.iter().filter(|t| t.status == TaskStatus::Queued).map(|t| (t.created_ms, t.id)).collect();
            q.sort();
            (q, st.settings.max_concurrent.max(1))
        };
        let mut jobs = self.jobs.lock().unwrap();
        for (_, id) in queued {
            if jobs.len() >= max {
                break;
            }
            if jobs.contains_key(&id) {
                continue;
            }
            let token = self.next_token.fetch_add(1, Ordering::Relaxed);
            let progress = Arc::new(Progress::default());
            let me = self.clone();
            let p = progress.clone();
            let handle = self.rt.spawn(async move { me.execute(id, token, p).await });
            jobs.insert(id, Running { token, abort: handle.abort_handle(), progress, last_bytes: 0 });
        }
    }

    /// 4 Hz: folds engine progress into tasks and smooths speed (audit A7).
    fn sample(&self) {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.is_empty() {
            return;
        }
        let mut st = self.state.lock().unwrap();
        let mut changed = false;
        for (id, job) in jobs.iter_mut() {
            let Some(t) = st.tasks.iter_mut().find(|t| t.id == *id) else { continue };
            let bytes = job.progress.bytes.load(Ordering::Relaxed);
            let delta = bytes.saturating_sub(job.last_bytes);
            job.last_bytes = bytes;
            if t.status == TaskStatus::Downloading && t.media.as_ref().is_none_or(|m| m.ytdlp_format.is_none()) {
                let inst = delta as f64 / 0.25;
                t.speed = if t.speed == 0.0 { inst } else { t.speed * 0.7 + inst * 0.3 };
                if t.speed < 1.0 {
                    t.speed = 0.0;
                }
                let segs = job.progress.segments.lock().unwrap();
                if !segs.is_empty() {
                    t.received_bytes = segs.iter().map(|s| s.received.min(s.len())).sum();
                    t.segments = segs.clone();
                }
                t.connections = job.progress.connections.load(Ordering::Relaxed);
                changed = true;
            }
            let logs: Vec<String> = std::mem::take(&mut *job.progress.log.lock().unwrap());
            for l in logs {
                t.push_log(l);
                changed = true;
            }
        }
        if changed {
            st.version += 1;
            st.dirty = true;
        }
    }

    fn save_if_dirty(&self) {
        let tasks = {
            let mut st = self.state.lock().unwrap();
            if !st.dirty {
                return;
            }
            st.dirty = false;
            st.tasks.clone()
        };
        self.store.save("tasks.json", &tasks);
    }

    fn emit(&self, e: Event) {
        self.state.lock().unwrap().events.push(e);
    }

    async fn execute(self: Arc<Self>, id: Uuid, token: u64, progress: Arc<Progress>) {
        let mut re_resolved = false;
        let mut restarted = false;
        loop {
            match self.run_once(id, &progress).await {
                Ok(()) => break,
                Err(Failure::Engine(EngineError::ValidatorChanged)) if !restarted => {
                    restarted = true;
                    self.update(id, |t| {
                        if let Some(p) = t.part_path() {
                            let _ = std::fs::remove_file(p);
                        }
                        t.segments.clear();
                        t.received_bytes = 0;
                        t.push_log(EngineError::ValidatorChanged.to_string());
                    });
                    progress.segments.lock().unwrap().clear();
                }
                // A signed media URL expired: resolve the post again and continue (audit A4).
                Err(Failure::Engine(EngineError::Http(403 | 404 | 410)))
                    if !re_resolved && self.with_task(id, |t| t.kind == TaskKind::Media).unwrap_or(false) =>
                {
                    re_resolved = true;
                    self.update(id, |t| {
                        t.fetch_url = None;
                        t.push_log("The media link expired; resolving the post again");
                    });
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.update(id, |t| {
                        t.status = TaskStatus::Failed;
                        t.error = Some(msg.clone());
                        t.speed = 0.0;
                        t.connections = 0;
                        t.push_log(format!("Failed: {msg}"));
                    });
                    if let Some(t) = self.task(id) {
                        self.emit(Event::Failed(t));
                    }
                    break;
                }
            }
        }
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.get(&id).is_some_and(|j| j.token == token) {
                jobs.remove(&id);
            }
        }
        self.pump();
    }

    async fn run_once(self: &Arc<Self>, id: Uuid, progress: &Arc<Progress>) -> Result<(), Failure> {
        let settings = self.settings();
        let Some(mut t) = self.task(id) else { return Ok(()) };

        // 1. Resolve the post.
        if t.kind == TaskKind::Media && t.fetch_url.is_none() && t.media.as_ref().is_none_or(|m| m.ytdlp_format.is_none()) {
            self.update(id, |t| t.status = TaskStatus::Resolving);
            self.resolve(id, &settings).await?;
            t = self.task(id).ok_or(Failure::Io("removed".into()))?;
        }
        if let Some(fmt) = t.media.as_ref().and_then(|m| m.ytdlp_format.clone()) {
            return self.run_ytdlp(id, &fmt, &settings).await;
        }
        let fetch = t.fetch_url.clone().unwrap_or_else(|| t.source.clone());
        let headers = t.media.as_ref().map(|m| m.headers.clone()).unwrap_or_default();
        let client = engine::http_client(&settings.user_agent);

        // 2. Probe; validate resume state (audit A5).
        self.update(id, |t| t.status = TaskStatus::Probing);
        let probe = {
            let mut attempt = 0u32;
            loop {
                match engine::probe(&client, &fetch, &headers).await {
                    Ok(p) => break p,
                    Err(e) if e.is_transient() && attempt < 5 => {
                        let delay = Duration::from_millis(500 << attempt);
                        self.update(id, |t| {
                            t.status = TaskStatus::Retrying;
                            t.push_log(format!("Probe: {e} Retry {} in {:.1}s", attempt + 1, delay.as_secs_f64()));
                        });
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };
        if !t.segments.is_empty() {
            let old = &t.validators;
            let changed = (old.length.is_some() && probe.length != old.length)
                || (old.etag.is_some() && probe.validators.etag.is_some() && old.etag != probe.validators.etag)
                || (old.etag.is_none() && old.last_modified.is_some() && probe.validators.last_modified != old.last_modified);
            if changed || !probe.accepts_ranges {
                let why = if changed { "The remote file changed; restarting" } else { "The server no longer supports resume; restarting" };
                if let Some(p) = t.part_path() {
                    let _ = std::fs::remove_file(p);
                }
                self.update(id, |t| {
                    t.segments.clear();
                    t.received_bytes = 0;
                    t.push_log(why);
                });
                t.segments.clear();
            }
        }

        // 3. Name and destination.
        std::fs::create_dir_all(&t.directory)?;
        if t.file_name.is_none() {
            let name = match t.kind {
                TaskKind::Media => media_file_name(&t, probe.file_name.as_deref().and_then(|n| util::split_ext(n).1).as_deref()),
                TaskKind::Http => util::sanitize(probe.file_name.as_deref().unwrap_or(&t.display_name), 180),
            };
            let reserved: HashSet<String> =
                self.state.lock().unwrap().tasks.iter().filter(|o| o.id != id).filter_map(|o| o.file_name.clone()).collect();
            let unique = util::unique_path(&t.directory, &name, &reserved, true);
            let fname = unique.file_name().unwrap().to_string_lossy().into_owned();
            self.update(id, |t| {
                if t.kind == TaskKind::Http {
                    t.display_name = fname.clone();
                }
                t.file_name = Some(fname);
            });
        }
        let pv = probe.validators.clone();
        self.update(id, |t| {
            t.total_bytes = probe.length;
            t.accepts_ranges = probe.accepts_ranges;
            t.validators = pv;
            t.fetch_url = Some(probe.final_url.clone());
            t.push_log(format!(
                "Probe: {}, ranges {}{}",
                fmt_bytes(probe.length),
                if probe.accepts_ranges { "yes" } else { "no" },
                if probe.validators.etag.is_some() { ", ETag" } else { "" }
            ));
        });
        let t = self.task(id).ok_or(Failure::Io("removed".into()))?;
        let part = t.part_path().unwrap();

        // Disk-space preflight.
        if let (Some(total), Some(free)) = (probe.length, util::free_space(&t.directory)) {
            let have: u64 = t.segments.iter().map(|s| s.received).sum();
            let need = total.saturating_sub(have);
            if need > free {
                return Err(EngineError::InsufficientSpace { needed: need, available: free }.into());
            }
        }

        // 4. Download.
        self.update(id, |t| t.status = TaskStatus::Downloading);
        let small = probe.length.unwrap_or(0) < settings.min_segment * 2;
        let conns = if small { 1 } else { t.max_connections.min(settings.per_host_cap).max(1) };
        let job = Job {
            url: probe.final_url.clone(),
            headers,
            part: part.clone(),
            total: probe.length,
            accepts_ranges: probe.accepts_ranges,
            validators: probe.validators.clone(),
            segments: t.segments.clone(),
            max_connections: conns,
            min_segment: settings.min_segment,
            max_retries: 8,
        };
        let segs = engine::download(client, job, self.limiter.clone(), progress.clone()).await?;

        // 5. Verify and finish.
        self.update(id, |t| {
            t.status = TaskStatus::Verifying;
            t.segments = segs.clone();
            t.speed = 0.0;
            t.connections = 0;
        });
        let size = std::fs::metadata(&part)?.len();
        if let Some(expected) = probe.length {
            if size != expected {
                return Err(EngineError::SizeMismatch { expected, got: size }.into());
            }
        }
        let digest = if settings.verify_sha256 { Some(engine::sha256_file(part.clone()).await?) } else { None };
        let dest = util::unique_path(&t.directory, t.file_name.as_deref().unwrap(), &HashSet::new(), false);
        std::fs::rename(&part, &dest)?;
        self.finish(id, dest, size, digest);
        Ok(())
    }

    fn finish(&self, id: Uuid, dest: PathBuf, size: u64, digest: Option<String>) {
        self.update(id, |t| {
            t.status = TaskStatus::Completed;
            t.completed_ms = Some(now_ms());
            t.file_name = dest.file_name().map(|f| f.to_string_lossy().into_owned());
            t.push_log(format!("Saved to {}", util::tilde(&dest)));
            t.final_path = Some(dest);
            t.total_bytes = Some(size);
            t.received_bytes = size;
            t.sha256 = digest;
            t.speed = 0.0;
            t.connections = 0;
        });
        if let Some(t) = self.task(id) {
            self.emit(Event::Finished(t));
        }
    }

    async fn resolve(self: &Arc<Self>, id: Uuid, settings: &Settings) -> Result<(), Failure> {
        let t = self.task(id).ok_or(Failure::Io("removed".into()))?;
        let info = t.media.clone().unwrap();
        let chain = ResolverChain::new(settings);
        let mut logs = Vec::new();
        let result = chain.resolve(&info.page_url, |l| logs.push(l)).await;
        self.update(id, |t| logs.iter().for_each(|l| t.push_log(l)));
        let items = result?;
        let quality = info.quality.unwrap_or(settings.quality);
        let total = items.len();
        let mut extra = Vec::new();
        for (n, item) in items.iter().enumerate() {
            let Some(fmt) = item.pick(quality) else { continue };
            if n == 0 {
                self.update(id, |t| apply(item, fmt, t));
            } else {
                // Posts with several videos: one task per video, already resolved.
                let mut more = DownloadTask::new(TaskKind::Media, &t.source, t.directory.clone(), t.max_connections, Some(info.clone()));
                more.created_ms = t.created_ms + n as u64;
                apply(item, fmt, &mut more);
                more.push_log(format!("Video {} of {total} from {}", n + 1, t.source));
                extra.push(more);
            }
        }
        if !extra.is_empty() {
            let mut st = self.state.lock().unwrap();
            let at = st.tasks.iter().position(|o| o.id == id).map_or(0, |i| i + 1);
            for (k, e) in extra.into_iter().enumerate() {
                st.tasks.insert(at + k, e);
            }
            st.version += 1;
            st.dirty = true;
            drop(st);
            self.pump();
        }
        Ok(())
    }

    async fn run_ytdlp(self: &Arc<Self>, id: Uuid, format: &str, settings: &Settings) -> Result<(), Failure> {
        let t = self.task(id).ok_or(Failure::Io("removed".into()))?;
        let y = ResolverChain::new(settings).ytdlp.ok_or(ResolveError::ToolMissing)?;
        std::fs::create_dir_all(&t.directory)?;
        self.update(id, |t| {
            t.status = TaskStatus::Downloading;
            t.connections = 1;
        });
        let base = util::split_ext(&media_file_name(&t, None)).0;
        let page = t.media.as_ref().unwrap().page_url.clone();
        let me = self.clone();
        let file = y
            .download(&page, format, &t.directory, &base, move |p: YtDlpProgress| {
                me.update(id, |t| {
                    t.received_bytes = p.downloaded;
                    if p.total.is_some() {
                        t.total_bytes = p.total;
                    }
                    t.speed = p.speed;
                    t.status = if p.merging { TaskStatus::Merging } else { TaskStatus::Downloading };
                });
            })
            .await?;
        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        self.finish(id, file, size, None);
        Ok(())
    }
}

fn apply(item: &MediaItem, fmt: &MediaFormat, t: &mut DownloadTask) {
    let prev = t.media.clone();
    let platform = if item.platform == Platform::Other { prev.as_ref().map_or(Platform::Other, |m| m.platform) } else { item.platform };
    let mut m = MediaInfo::new(platform, prev.as_ref().map_or(item.page_url.clone(), |m| m.page_url.clone()));
    m.title = item.title.clone();
    m.author = item.author.clone();
    m.thumbnail_url = item.thumbnail.clone();
    m.format_label = Some(fmt.label());
    m.width = fmt.width;
    m.height = fmt.height;
    m.duration = item.duration;
    m.ytdlp_format = fmt.ytdlp_format.clone();
    m.headers = fmt.headers.clone();
    m.quality = prev.and_then(|p| p.quality);
    t.media = Some(m);
    t.fetch_url = fmt.url.clone();
    let title = item.title.as_deref().map(|s| s.replace('\n', " ")).filter(|s| !s.trim().is_empty());
    t.display_name = match title {
        Some(s) => s.chars().take(120).collect(),
        None => format!("{} video by {}", platform.display_name(), item.author.as_deref().unwrap_or("unknown")),
    };
    t.push_log(format!("Selected {} via {}", fmt.label(), item.resolved_by));
}

/// "X - nasa - Liftoff of Artemis.mp4"
fn media_file_name(t: &DownloadTask, ext: Option<&str>) -> String {
    let m = t.media.as_ref();
    let author = m.and_then(|m| m.author.as_deref()).map(|a| a.trim_start_matches('@').to_owned());
    let title: String = t.display_name.chars().take(80).collect();
    let platform = m.map_or("Video", |m| m.platform.display_name()).to_owned();
    let base = [Some(platform), author, Some(title)].into_iter().flatten().collect::<Vec<_>>().join(" - ");
    let ext = ext.filter(|e| !e.is_empty() && e.len() <= 5).unwrap_or("mp4");
    format!("{}.{ext}", util::sanitize(&base, 150))
}

/// Opens the default store under the platform data directory.
pub fn default_store() -> Store {
    Store::at(util::data_dir())
}

pub fn is_media_link(s: &str) -> bool {
    classify(s).is_some()
}


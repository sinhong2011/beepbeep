// SPDX-License-Identifier: AGPL-3.0-or-later
//! Segmented HTTP/HTTPS engine (plan §17–18): probe, plan, fetch with worker reuse and work stealing,
//! per-segment retry with backoff, If-Range validation, a shared token bucket, positional writes.

use crate::domain::{Segment, Validators};
use futures_util::StreamExt;
use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, StatusCode};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    Http(u16),
    ValidatorChanged,
    InsufficientSpace { needed: u64, available: u64 },
    SizeMismatch { expected: u64, got: u64 },
    Network(String),
    Io(String),
}

impl EngineError {
    /// Transient errors are retried; the rest fail the task.
    pub fn is_transient(&self) -> bool {
        match self {
            EngineError::Http(c) => matches!(c, 408 | 425 | 429) || *c >= 500,
            EngineError::Network(_) => true,
            _ => false,
        }
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::util::fmt_bytes;
        match self {
            EngineError::Http(401) => write!(f, "The server requires a login (HTTP 401)."),
            EngineError::Http(403) => write!(f, "Access denied (HTTP 403). The link may have expired."),
            EngineError::Http(404) => write!(f, "File not found (HTTP 404)."),
            EngineError::Http(410) => write!(f, "The link has expired (HTTP 410)."),
            EngineError::Http(416) => write!(f, "The server rejected the requested range (HTTP 416)."),
            EngineError::Http(429) => write!(f, "Too many requests (HTTP 429). Try again later."),
            EngineError::Http(c) => write!(f, "Server error (HTTP {c})."),
            EngineError::ValidatorChanged => write!(f, "The file changed on the server; restarting from the beginning."),
            EngineError::InsufficientSpace { needed, available } => write!(
                f,
                "Not enough disk space: needs {}, {} free.",
                fmt_bytes(Some(*needed)),
                fmt_bytes(Some(*available))
            ),
            EngineError::SizeMismatch { expected, got } => write!(
                f,
                "Downloaded size {} does not match the expected {}.",
                fmt_bytes(Some(*got)),
                fmt_bytes(Some(*expected))
            ),
            EngineError::Network(s) => write!(f, "{s}"),
            EngineError::Io(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<reqwest::Error> for EngineError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(s) = e.status() {
            return EngineError::Http(s.as_u16());
        }
        let msg = if e.is_timeout() {
            "The connection timed out.".to_owned()
        } else if e.is_connect() {
            "Could not connect to the server.".to_owned()
        } else {
            let mut s = e.to_string();
            let mut src = std::error::Error::source(&e);
            while let Some(inner) = src {
                s = inner.to_string();
                src = inner.source();
            }
            s
        };
        EngineError::Network(crate::util::redact(&msg))
    }
}

pub fn http_client(user_agent: &str) -> Client {
    Client::builder()
        .user_agent(user_agent)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("http client")
}

fn header_map(headers: &[(String, String)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        if let (Ok(k), Ok(v)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            map.insert(k, v);
        }
    }
    map
}

// ---------------------------------------------------------------------------------------------
// Probe

#[derive(Debug, Clone)]
pub struct Probe {
    pub final_url: String,
    pub length: Option<u64>,
    pub accepts_ranges: bool,
    pub validators: Validators,
    pub file_name: Option<String>,
    pub mime: Option<String>,
}

/// One `GET Range: bytes=0-0` instead of HEAD: signed CDN URLs often reject HEAD, and
/// a 206 with Content-Range gives size and range support in one round trip.
pub async fn probe(client: &Client, url: &str, headers: &[(String, String)]) -> Result<Probe, EngineError> {
    let resp = client
        .get(url)
        .headers(header_map(headers))
        .header(header::RANGE, "bytes=0-0")
        .header(header::ACCEPT_ENCODING, "identity")
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(EngineError::Http(status.as_u16()));
    }
    let h = resp.headers();
    let get = |k: header::HeaderName| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let mut length = None;
    let mut ranges = false;
    if status == StatusCode::PARTIAL_CONTENT {
        if let Some(total) = get(header::CONTENT_RANGE).and_then(|cr| cr.rsplit('/').next().and_then(|t| t.parse().ok())) {
            length = Some(total);
            ranges = true;
        }
    } else if get(header::CONTENT_ENCODING).is_none() {
        length = resp.content_length().filter(|n| *n > 0);
    }
    let final_url = resp.url().to_string();
    let mime = get(header::CONTENT_TYPE).map(|m| m.split(';').next().unwrap_or("").trim().to_owned());
    let file_name = get(header::CONTENT_DISPOSITION)
        .and_then(|cd| disposition_filename(&cd))
        .or_else(|| crate::util::name_from_url(&final_url))
        .map(|n| with_mime_ext(n, mime.as_deref()));
    let validators = Validators { etag: get(header::ETAG), last_modified: get(header::LAST_MODIFIED), length };
    drop(resp); // closes the body without reading it
    Ok(Probe { final_url, length, accepts_ranges: ranges, validators, file_name, mime })
}

/// `filename*=UTF-8''…` (RFC 6266) or `filename="…"`.
pub fn disposition_filename(cd: &str) -> Option<String> {
    for part in cd.split(';').map(str::trim) {
        if let Some(v) = part.strip_prefix("filename*=").or_else(|| part.strip_prefix("FILENAME*=")) {
            let v = v.trim_matches('"');
            let enc = v.splitn(3, '\'').nth(2).unwrap_or(v);
            return Some(percent_encoding::percent_decode_str(enc).decode_utf8_lossy().into_owned());
        }
    }
    for part in cd.split(';').map(str::trim) {
        if let Some(v) = part.strip_prefix("filename=").or_else(|| part.strip_prefix("FILENAME=")) {
            let v = v.trim().trim_matches('"');
            if !v.is_empty() {
                return Some(v.to_owned());
            }
        }
    }
    None
}

fn with_mime_ext(name: String, mime: Option<&str>) -> String {
    if crate::util::split_ext(&name).1.is_some() {
        return name;
    }
    let ext = mime.and_then(|m| mime_guess::get_mime_extensions_str(m)).and_then(|e| e.first().copied());
    match ext {
        Some(e) => format!("{name}.{e}"),
        None => name,
    }
}

// ---------------------------------------------------------------------------------------------
// Planner (pure)

pub fn plan(length: u64, connections: usize, min_segment: u64) -> Vec<Segment> {
    if length == 0 {
        return vec![Segment::new(0, 0)];
    }
    let by_min = (length / min_segment.max(1)).max(1) as usize;
    let n = connections.clamp(1, by_min.max(1)) as u64;
    let base = length / n;
    let mut out = Vec::with_capacity(n as usize);
    let mut start = 0;
    for i in 0..n {
        let end = if i == n - 1 { length - 1 } else { start + base - 1 };
        out.push(Segment::new(start, end));
        start = end + 1;
    }
    out
}

/// Work stealing: halve the unfinished segment with the most remaining bytes; the new tail is appended.
pub fn split_largest(segs: &mut Vec<Segment>, min_segment: u64) -> Option<usize> {
    let idx = (0..segs.len()).filter(|&i| !segs[i].is_done() && segs[i].end != u64::MAX).max_by_key(|&i| segs[i].remaining())?;
    let s = segs[idx];
    if s.remaining() < min_segment * 2 {
        return None;
    }
    let mid = s.cursor() + s.remaining() / 2;
    segs[idx].end = mid - 1;
    segs.push(Segment::new(mid, s.end));
    Some(segs.len() - 1)
}

// ---------------------------------------------------------------------------------------------
// Rate limiter: one token bucket shared by all tasks.

pub struct RateLimiter {
    inner: Mutex<(f64, f64, Instant)>, // rate, tokens, last
}

impl RateLimiter {
    pub fn new(bps: u64) -> Self {
        Self { inner: Mutex::new((bps as f64, bps as f64, Instant::now())) }
    }
    pub fn set_rate(&self, bps: u64) {
        *self.inner.lock().unwrap() = (bps as f64, bps as f64, Instant::now());
    }
    /// Takes `n` bytes; returns how long the caller should wait.
    pub fn take(&self, n: usize) -> Duration {
        let mut g = self.inner.lock().unwrap();
        let (rate, ref mut tokens, ref mut last) = *g;
        if rate <= 0.0 {
            return Duration::ZERO;
        }
        let now = Instant::now();
        *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * rate).min(rate);
        *last = now;
        *tokens -= n as f64;
        if *tokens < 0.0 { Duration::from_secs_f64(-*tokens / rate) } else { Duration::ZERO }
    }
}

// ---------------------------------------------------------------------------------------------
// Downloader

pub struct Job {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub part: PathBuf,
    pub total: Option<u64>,
    pub accepts_ranges: bool,
    pub validators: Validators,
    pub segments: Vec<Segment>,
    pub max_connections: usize,
    pub min_segment: u64,
    pub max_retries: u32,
}

/// Live state shared with the caller for progress sampling.
#[derive(Default)]
pub struct Progress {
    pub segments: Mutex<Vec<Segment>>,
    pub bytes: AtomicU64,
    pub connections: AtomicUsize,
    pub log: Mutex<Vec<String>>,
}

impl Progress {
    fn log(&self, s: String) {
        self.log.lock().unwrap().push(s);
    }
}

struct Shared {
    client: Client,
    job: Job,
    file: File,
    limiter: Arc<RateLimiter>,
    progress: Arc<Progress>,
    ranged: bool,
}

/// Runs a download until every segment is complete. Dropping the future (abort) stops all workers;
/// the segment map in `progress` is always consistent with bytes on disk.
pub async fn download(client: Client, mut job: Job, limiter: Arc<RateLimiter>, progress: Arc<Progress>) -> Result<Vec<Segment>, EngineError> {
    let ranged = job.accepts_ranges && job.total.unwrap_or(0) > 0;
    if !ranged {
        // No range support: a single stream from zero; partial bytes are useless.
        job.segments = vec![Segment::new(0, job.total.map(|t| t.saturating_sub(1)).unwrap_or(u64::MAX))];
        let _ = std::fs::remove_file(&job.part);
    } else if job.segments.is_empty() {
        job.segments = plan(job.total.unwrap(), job.max_connections, job.min_segment);
    }
    let file = open_part(&job.part, if ranged { job.total } else { None }).map_err(|e| {
        EngineError::Io(format!("Cannot open {}: {e}", job.part.file_name().unwrap_or_default().to_string_lossy()))
    })?;
    *progress.segments.lock().unwrap() = job.segments.clone();
    progress.log(if ranged {
        format!(
            "Starting {} segment(s), up to {} connections",
            job.segments.iter().filter(|s| !s.is_done()).count(),
            job.max_connections
        )
    } else {
        "Server does not support ranges; using one connection".into()
    });

    let shared = Arc::new(Shared { client, job, file, limiter, progress: progress.clone(), ranged });
    let mut workers: JoinSet<(usize, Result<(), EngineError>)> = JoinSet::new();
    let mut busy: Vec<usize> = Vec::new();

    loop {
        // Fill free connections: idle unfinished segments first, then steal tails.
        {
            let mut segs = shared.progress.segments.lock().unwrap();
            if segs.iter().all(Segment::is_done) && busy.is_empty() {
                let out = segs.clone();
                drop(segs);
                shared.file.sync_data().ok();
                return Ok(out);
            }
            for i in 0..segs.len() {
                if busy.len() >= shared.job.max_connections {
                    break;
                }
                if !segs[i].is_done() && !busy.contains(&i) {
                    busy.push(i);
                    let s = shared.clone();
                    workers.spawn(async move { (i, worker(s, i).await) });
                }
            }
            while ranged && busy.len() < shared.job.max_connections {
                let Some(i) = split_largest(&mut segs, shared.job.min_segment) else { break };
                busy.push(i);
                let s = shared.clone();
                workers.spawn(async move { (i, worker(s, i).await) });
            }
            shared.progress.connections.store(busy.len(), Ordering::Relaxed);
        }
        let Some(done) = workers.join_next().await else { continue };
        let (i, result) = done.map_err(|e| EngineError::Io(e.to_string()))?;
        busy.retain(|&b| b != i);
        result?; // fatal error: dropping `workers` aborts the rest
    }
}

fn open_part(path: &Path, presize: Option<u64>) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let f = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
    if let Some(total) = presize {
        if f.metadata()?.len() != total {
            f.set_len(total)?; // positional writes never leave holes past EOF
        }
    }
    Ok(f)
}

#[cfg(unix)]
fn write_at(f: &File, buf: &[u8], off: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.write_all_at(buf, off)
}

#[cfg(windows)]
fn write_at(f: &File, mut buf: &[u8], mut off: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_write(buf, off)?;
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

/// Downloads segment `i` until it is done (its end may shrink while running when another worker steals the tail).
async fn worker(s: Arc<Shared>, i: usize) -> Result<(), EngineError> {
    let mut attempt = 0u32;
    loop {
        let before = s.progress.segments.lock().unwrap()[i].received;
        let result = fetch_segment(&s, i).await;
        // A request that moved the cursor resets the backoff: the server is working, just flaky.
        if s.progress.segments.lock().unwrap()[i].received > before {
            attempt = 0;
        }
        match result {
            Ok(()) => return Ok(()),
            Err(e) if e.is_transient() && attempt < s.job.max_retries => {
                let delay = Duration::from_millis((250u64 << attempt).min(8_000));
                s.progress.log(format!("Segment {}: {e} Retry {} in {:.1}s", i + 1, attempt + 1, delay.as_secs_f64()));
                attempt += 1;
                if !s.ranged {
                    s.progress.segments.lock().unwrap()[i].received = 0;
                    s.file.set_len(0).ok();
                }
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}

const STALL: Duration = Duration::from_secs(10);

async fn fetch_segment(s: &Shared, i: usize) -> Result<(), EngineError> {
    let (cursor, end) = {
        let g = s.progress.segments.lock().unwrap();
        (g[i].cursor(), g[i].end)
    };
    let mut req = s.client.get(&s.job.url).headers(header_map(&s.job.headers)).header(header::ACCEPT_ENCODING, "identity");
    if s.ranged {
        req = req.header(header::RANGE, format!("bytes={cursor}-{end}"));
        if let Some(v) = s.job.validators.if_range() {
            req = req.header(header::IF_RANGE, v);
        }
    }
    let resp = req.send().await?;
    let status = resp.status();
    if s.ranged {
        if status == StatusCode::OK {
            // If-Range failed or Range was ignored; only safe at byte 0 of a single segment.
            let single = s.progress.segments.lock().unwrap().len() == 1;
            if !(cursor == 0 && single) {
                return Err(EngineError::ValidatorChanged);
            }
        } else if status != StatusCode::PARTIAL_CONTENT {
            return Err(EngineError::Http(status.as_u16()));
        }
    } else if !status.is_success() {
        return Err(EngineError::Http(status.as_u16()));
    }

    let mut stream = resp.bytes_stream();
    loop {
        // A connection that sends nothing for STALL is dropped and retried (plan §17: drop slow workers).
        let next = match tokio::time::timeout(STALL, stream.next()).await {
            Ok(n) => n,
            Err(_) => return Err(EngineError::Network("The connection stalled.".into())),
        };
        let Some(chunk) = next else { break };
        let chunk = chunk?;
        // Re-read the segment: a steal may have moved its end.
        let (off, allowed) = {
            let g = s.progress.segments.lock().unwrap();
            let seg = g[i];
            (seg.cursor(), seg.end.saturating_sub(seg.cursor()).saturating_add(1))
        };
        if allowed == 0 {
            return Ok(());
        }
        let n = (chunk.len() as u64).min(allowed) as usize;
        write_at(&s.file, &chunk[..n], off).map_err(|e| EngineError::Io(format!("Disk write failed: {e}")))?;
        let done = {
            let mut g = s.progress.segments.lock().unwrap();
            g[i].received += n as u64;
            g[i].is_done()
        };
        s.progress.bytes.fetch_add(n as u64, Ordering::Relaxed);
        if done {
            return Ok(());
        }
        let wait = s.limiter.take(n);
        if !wait.is_zero() {
            tokio::time::sleep(wait.min(Duration::from_secs(2))).await;
        }
    }
    // Stream ended.
    let mut g = s.progress.segments.lock().unwrap();
    if g[i].end == u64::MAX {
        // Unknown length: a clean end closes the open segment.
        let len = g[i].received;
        g[i].end = g[i].start + len.max(1) - 1;
        if len == 0 {
            g[i].received = 1; // empty body; mark done
        }
        return Ok(());
    }
    if g[i].is_done() { Ok(()) } else { Err(EngineError::Network("The connection closed early.".into())) }
}

/// SHA-256 of a file, off the async threads.
pub async fn sha256_file(path: PathBuf) -> std::io::Result<String> {
    tokio::task::spawn_blocking(move || {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut f = File::open(path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 4 << 20];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    })
    .await
    .map_err(std::io::Error::other)?
}

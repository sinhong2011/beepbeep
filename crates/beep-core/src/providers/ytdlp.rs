// SPDX-License-Identifier: AGPL-3.0-or-later
//! yt-dlp as a supervised external program (plan §14): metadata with `-J`, downloads and merges as a
//! child process with parsed progress and cancellation (the child is killed when the future is dropped).

use super::{MediaFormat, MediaItem, ResolveError, ResolveFuture, VideoProvider, classify};
use crate::domain::Platform;
use crate::settings::CookieBrowser;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct YtDlp {
    pub path: PathBuf,
    pub ffmpeg: Option<PathBuf>,
    pub cookies: CookieBrowser,
}

#[derive(Debug, Clone, Default)]
pub struct YtDlpProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: f64,
    pub merging: bool,
}

impl YtDlp {
    fn command(&self) -> Command {
        let mut c = Command::new(&self.path);
        c.arg("--no-warnings").arg("--no-playlist");
        if let Some(b) = self.cookies.arg() {
            c.arg("--cookies-from-browser").arg(b);
        }
        if let Some(f) = &self.ffmpeg {
            c.arg("--ffmpeg-location").arg(f);
        }
        c.kill_on_drop(true).stdin(Stdio::null());
        c
    }

    pub fn parse_entry(&self, e: &Value, fallback_page: &str) -> Option<MediaItem> {
        let mut formats = Vec::new();
        let mut sides = BTreeSet::new();
        let mut has_audio = false;
        for f in e["formats"].as_array().into_iter().flatten() {
            let vcodec = f["vcodec"].as_str().unwrap_or("none");
            let acodec = f["acodec"].as_str().unwrap_or("none");
            let proto = f["protocol"].as_str().unwrap_or("");
            let w = f["width"].as_u64().map(|v| v as u32);
            let h = f["height"].as_u64().map(|v| v as u32);
            if vcodec != "none" {
                if let Some(h) = h {
                    sides.insert(w.map_or(h, |w| w.min(h)));
                }
            }
            has_audio |= vcodec == "none" && acodec != "none";
            // Progressive HTTP renditions go to the segmented engine directly.
            if vcodec != "none" && acodec != "none" && (proto == "https" || proto == "http") {
                if let Some(u) = f["url"].as_str() {
                    let mut mf = MediaFormat::direct(f["format_id"].as_str().unwrap_or("f"), u, w, h, f["tbr"].as_f64().map(|t| (t * 1000.0) as u64));
                    mf.ext = f["ext"].as_str().unwrap_or("mp4").to_owned();
                    mf.headers = headers(&f["http_headers"]);
                    formats.push(mf);
                }
            }
        }
        // Renditions yt-dlp fetches and merges itself.
        let merge = self.ffmpeg.is_some();
        for side in sides {
            let sel = if merge { format!("bv*[height<={side}]+ba/b[height<={side}]/b") } else { format!("b[height<={side}]/b") };
            formats.push(MediaFormat {
                id: format!("ytdlp-{side}"),
                url: None,
                ytdlp_format: Some(sel),
                ext: "mp4".into(),
                width: None,
                height: Some(side),
                bitrate: None,
                audio_only: false,
                headers: Vec::new(),
            });
        }
        if has_audio {
            formats.push(MediaFormat {
                id: "ytdlp-audio".into(),
                url: None,
                ytdlp_format: Some("ba[ext=m4a]/ba".into()),
                ext: "m4a".into(),
                width: None,
                height: None,
                bitrate: None,
                audio_only: true,
                headers: Vec::new(),
            });
        }
        if formats.is_empty() {
            if let Some(u) = e["url"].as_str() {
                let mut mf = MediaFormat::direct("direct", u, e["width"].as_u64().map(|v| v as u32), e["height"].as_u64().map(|v| v as u32), None);
                mf.headers = headers(&e["http_headers"]);
                formats.push(mf);
            }
        }
        if formats.is_empty() {
            return None;
        }
        let page = e["webpage_url"].as_str().unwrap_or(fallback_page).to_owned();
        let platform = classify(&page).or_else(|| classify(fallback_page)).unwrap_or(Platform::Other);
        let title = e["description"].as_str().filter(|d| !d.trim().is_empty()).or(e["title"].as_str());
        Some(MediaItem {
            id: e["id"].as_str().unwrap_or("video").to_owned(),
            platform,
            page_url: page,
            title: title.map(|t| t.chars().take(200).collect()),
            author: e["uploader_id"].as_str().map(|u| format!("@{u}")).or(e["uploader"].as_str().map(str::to_owned)),
            thumbnail: e["thumbnail"].as_str().map(str::to_owned),
            duration: e["duration"].as_f64(),
            formats,
            resolved_by: "yt-dlp",
        })
    }

    /// Downloads with yt-dlp; returns the final file path.
    pub async fn download(
        &self,
        page_url: &str,
        format: &str,
        dir: &Path,
        base_name: &str,
        mut on_progress: impl FnMut(YtDlpProgress),
    ) -> Result<PathBuf, ResolveError> {
        let template = dir.join(format!("{} [%(id)s].%(ext)s", crate::util::sanitize(base_name, 120)));
        let mut cmd = self.command();
        cmd.args(["-f", format, "--merge-output-format", "mp4", "--newline", "--progress", "--no-simulate"])
            .arg("-o")
            .arg(&template)
            .args(["--print", "after_move:BEEPFILE %(filepath)s"])
            .args([
                "--progress-template",
                "download:BEEPPROG %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s",
            ])
            .arg(page_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| ResolveError::Failed(format!("Could not start yt-dlp: {e}")))?;
        let stderr = child.stderr.take().unwrap();
        let err_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut buf = String::new();
            while let Ok(Some(l)) = lines.next_line().await {
                buf.push_str(&l);
                buf.push('\n');
            }
            buf
        });
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut final_path = None;
        let mut state = YtDlpProgress::default();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(rest) = line.strip_prefix("BEEPPROG ") {
                let p: Vec<&str> = rest.split_whitespace().collect();
                let num = |s: Option<&&str>| s.and_then(|v| v.parse::<f64>().ok());
                state.downloaded = num(p.first()).unwrap_or(0.0) as u64;
                state.total = num(p.get(1)).or(num(p.get(2))).map(|t| t as u64);
                state.speed = num(p.get(3)).unwrap_or(0.0);
                on_progress(state.clone());
            } else if let Some(path) = line.strip_prefix("BEEPFILE ") {
                final_path = Some(PathBuf::from(path.trim()));
            } else if line.contains("[Merger]") {
                state.merging = true;
                on_progress(state.clone());
            }
        }
        let status = child.wait().await.map_err(|e| ResolveError::Failed(e.to_string()))?;
        let stderr = err_task.await.unwrap_or_default();
        match final_path {
            Some(p) if status.success() => Ok(p),
            _ => Err(classify_error(&stderr, classify(page_url))),
        }
    }
}

fn headers(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned()))).collect())
        .unwrap_or_default()
}

pub fn classify_error(stderr: &str, platform: Option<Platform>) -> ResolveError {
    let s = stderr.to_ascii_lowercase();
    if s.contains("login") || s.contains("cookies") || s.contains("private") || s.contains("authentication") {
        return ResolveError::LoginRequired(platform.unwrap_or(Platform::Other));
    }
    if s.contains("429") || s.contains("rate-limit") || s.contains("rate limit") {
        return ResolveError::RateLimited;
    }
    if s.contains("unsupported url") {
        return ResolveError::Unsupported;
    }
    if s.contains("404") || s.contains("not found") || s.contains("does not exist") {
        return ResolveError::NotFound;
    }
    if s.contains("no video") {
        return ResolveError::NoVideo;
    }
    let line = stderr.lines().rev().find(|l| l.contains("ERROR")).unwrap_or(stderr.trim());
    ResolveError::Failed(crate::util::redact(line.trim_start_matches("ERROR: ")))
}

impl VideoProvider for YtDlp {
    fn name(&self) -> &'static str {
        "yt-dlp"
    }
    fn can_handle(&self, url: &url::Url) -> bool {
        matches!(url.scheme(), "http" | "https")
    }
    fn resolve<'a>(&'a self, url: &'a url::Url) -> ResolveFuture<'a> {
        Box::pin(async move {
            let out = self
                .command()
                .arg("-J")
                .arg(url.as_str())
                .output()
                .await
                .map_err(|e| ResolveError::Failed(format!("Could not start yt-dlp: {e}")))?;
            if !out.status.success() {
                return Err(classify_error(&String::from_utf8_lossy(&out.stderr), classify(url.as_str())));
            }
            let json: Value = serde_json::from_slice(&out.stdout).map_err(|_| ResolveError::Failed("yt-dlp returned unreadable metadata".into()))?;
            let entries = match json["entries"].as_array() {
                Some(e) => e.clone(),
                None => vec![json],
            };
            let items: Vec<MediaItem> = entries.iter().filter_map(|e| self.parse_entry(e, url.as_str())).collect();
            if items.is_empty() { Err(ResolveError::NoVideo) } else { Ok(items) }
        })
    }
}

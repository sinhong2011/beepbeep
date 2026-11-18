// SPDX-License-Identifier: AGPL-3.0-or-later
//! BeepBeep's native side: the download service (`beep-core`) exported to the TSX screens as the
//! `Engine` module of `beepbeep/native` (plan §22: Rust for the core and high-frequency state, TSX for
//! screens). The screens poll `Engine.snapshot(version)` at 4 Hz and call commands; everything that
//! touches the network or the disk stays here.

mod glyph;

pub use glyph::{Glyph, GlyphProps};

use beep_core::util::{fmt_bytes, fmt_duration, fmt_eta, fmt_speed, tilde};
use beep_core::{DownloadTask, Event, Quality, Service, TaskStatus, providers, util};
use nsh_core::prelude::*;
use serde::Serialize;
use std::sync::OnceLock;
use uuid::Uuid;

/// The crate's UI set: what `App.tsx` imports from `beepbeep/native`.
#[native_module("beepbeep/native")]
fn native(registry: &mut Registry) {
    registry.register::<Glyph>().module::<Engine>();
}

/// The user's one queue: owned here when BeepBeep starts first, else attached to the process that owns
/// it (another BeepBeep, or `beep mcp`), so downloads added anywhere show up here live.
fn manager() -> &'static Service {
    static M: OnceLock<Service> = OnceLock::new();
    M.get_or_init(|| Service::connect_or_start(util::data_dir(), None))
}

fn uuid(id: &str) -> Option<Uuid> {
    Uuid::parse_str(id).ok()
}

/// A download as the screens show it: display strings are formatted here, once.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskView {
    id: String,
    status: &'static str,
    status_label: &'static str,
    active: bool,
    title: String,
    source: String,
    host: String,
    platform: &'static str,
    platform_name: &'static str,
    is_media: bool,
    author: Option<String>,
    quality: Option<String>,
    duration: Option<String>,
    thumbnail: Option<String>,
    /// 0…1, or null while the size is unknown.
    progress: Option<f64>,
    size_text: String,
    received_text: String,
    speed: f64,
    speed_text: String,
    eta_text: String,
    connections: usize,
    error: Option<String>,
    final_path: Option<String>,
    file_exists: bool,
    folder: String,
    file_name: Option<String>,
    resumable: bool,
    sha256: Option<String>,
    /// Each segment's share of the file and how much of it is done, in file order.
    segments: Vec<(f64, f64)>,
    log: Vec<String>,
    created_ms: f64,
}

fn platform_id(t: &DownloadTask) -> &'static str {
    use beep_core::Platform as P;
    match t.platform() {
        P::X => "x",
        P::Threads => "threads",
        P::Instagram => "instagram",
        P::Other => "video",
        P::Web => "file",
    }
}

fn status_id(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Queued => "queued",
        TaskStatus::Resolving => "resolving",
        TaskStatus::Probing => "probing",
        TaskStatus::Downloading => "downloading",
        TaskStatus::Retrying => "retrying",
        TaskStatus::Merging => "merging",
        TaskStatus::Verifying => "verifying",
        TaskStatus::Paused => "paused",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn view_of(t: &DownloadTask) -> TaskView {
    let m = t.media.as_ref();
    let mut segs = t.segments.clone();
    segs.sort_by_key(|s| s.start);
    let total: u64 = segs.iter().map(|s| s.len().min(u64::MAX / 4)).sum::<u64>().max(1);
    TaskView {
        id: t.id.to_string(),
        status: status_id(t.status),
        status_label: t.status.label(),
        active: t.status.is_active(),
        title: t.display_name.clone(),
        source: t.source.clone(),
        host: t.host(),
        platform: platform_id(t),
        platform_name: t.platform().display_name(),
        is_media: m.is_some(),
        author: m.and_then(|m| m.author.clone()),
        quality: m.and_then(|m| m.format_label.clone()),
        duration: m.and_then(|m| fmt_duration(m.duration)),
        thumbnail: m.and_then(|m| m.thumbnail_url.clone()),
        progress: t.total_bytes.filter(|t| *t > 0).map(|_| if t.status == TaskStatus::Completed { 1.0 } else { t.progress() }),
        size_text: fmt_bytes(t.total_bytes),
        received_text: fmt_bytes(Some(t.received_bytes)),
        speed: t.speed,
        speed_text: if t.status.is_active() { fmt_speed(t.speed) } else { String::new() },
        eta_text: if t.status == TaskStatus::Downloading { fmt_eta(t.eta()) } else { String::new() },
        connections: t.connections,
        error: t.error.clone(),
        final_path: t.final_path.as_ref().map(|p| p.display().to_string()),
        file_exists: t.final_path.as_ref().is_some_and(|p| p.exists()),
        folder: tilde(&t.directory),
        file_name: t.file_name.clone(),
        resumable: t.accepts_ranges,
        sha256: t.sha256.clone(),
        segments: segs
            .iter()
            .map(|s| {
                let len = s.len().min(u64::MAX / 4);
                (len as f64 / total as f64, if len == 0 { 1.0 } else { (s.received as f64 / len as f64).min(1.0) })
            })
            .collect(),
        log: t.log.iter().rev().take(40).map(|l| l.message.clone()).collect(),
        created_ms: t.created_ms as f64,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotView {
    version: f64,
    tasks: Vec<TaskView>,
    settings: SettingsView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    folder: String,
    folder_path: String,
    max_concurrent: usize,
    connections: usize,
    speed_limit: f64,
    speed_limit_text: String,
    notify: bool,
    verify_sha256: bool,
    quality: &'static str,
    cookie_browser: &'static str,
    category_shortcuts: bool,
    free_text: String,
}

fn quality_id(q: Quality) -> &'static str {
    match q {
        Quality::Best => "best",
        Quality::P1080 => "1080",
        Quality::P720 => "720",
        Quality::P480 => "480",
        Quality::Audio => "audio",
    }
}

/// The download service: commands and a polled snapshot.
#[module]
pub struct Engine;

#[module]
impl Engine {
    /// The downloads and settings as JSON when anything changed since `version`; otherwise "".
    pub fn snapshot(version: f64) -> String {
        let Some(snap) = manager().snapshot_since(version as u64) else { return String::new() };
        let s = &snap.settings;
        let view = SnapshotView {
            version: snap.version as f64,
            tasks: snap.tasks.iter().map(view_of).collect(),
            settings: SettingsView {
                folder: tilde(&s.download_dir),
                folder_path: s.download_dir.display().to_string(),
                max_concurrent: s.max_concurrent,
                connections: s.connections_per_task,
                speed_limit: s.speed_limit as f64,
                speed_limit_text: if s.speed_limit == 0 { "Unlimited".into() } else { fmt_speed(s.speed_limit as f64) },
                notify: s.notify,
                verify_sha256: s.verify_sha256,
                quality: quality_id(s.quality),
                cookie_browser: s.cookie_browser.arg().unwrap_or("none"),
                category_shortcuts: s.category_shortcuts,
                free_text: fmt_bytes(util::free_space(&s.download_dir)),
            },
        };
        serde_json::to_string(&view).unwrap_or_default()
    }

    /// Adds every link found in `text`; `quality` is "best", "1080", "720", "480", "audio" or empty
    /// for the default. Returns the new downloads' ids, in the order pasted.
    pub fn add_links(text: String, quality: String) -> Vec<String> {
        manager().add_text(&text, Quality::parse(&quality)).into_iter().map(|id| id.to_string()).collect()
    }

    /// The http(s) links in `text`, in order, without repeats.
    pub fn extract_links(text: String) -> Vec<String> {
        util::extract_links(&text)
    }

    /// "x", "threads", "instagram" for a post link; "file" for anything else.
    pub fn link_kind(url: String) -> String {
        match providers::classify(&url) {
            Some(p) => p.display_name().to_lowercase(),
            None => "file".into(),
        }
    }

    /// Pauses a download; it resumes from where it stopped.
    pub fn pause(id: String) {
        if let Some(id) = uuid(&id) {
            manager().pause(id);
        }
    }

    /// Resumes a paused, failed or stopped download.
    pub fn resume(id: String) {
        if let Some(id) = uuid(&id) {
            manager().resume(id);
        }
    }

    /// Stops a download (it keeps its place in the list).
    pub fn stop(id: String) {
        if let Some(id) = uuid(&id) {
            manager().stop(id);
        }
    }

    /// Starts a finished download over.
    pub fn redownload(id: String) {
        if let Some(id) = uuid(&id) {
            manager().redownload(id);
        }
    }

    /// Removes a download from the list; with `delete_file`, returns its file's path to move to the Trash.
    pub fn remove(id: String, delete_file: bool) -> Option<String> {
        let id = uuid(&id)?;
        manager().remove(id, delete_file).map(|p| p.display().to_string())
    }

    /// Pauses everything running or waiting.
    pub fn pause_all() {
        manager().pause_all();
    }

    /// Resumes everything paused.
    pub fn resume_all() {
        manager().resume_all();
    }

    /// Removes completed and stopped downloads from the list (files stay).
    pub fn clear_finished() {
        manager().clear_finished();
    }

    /// Finished and failed downloads since the last call, as JSON: `[{kind, title, detail, id}]`.
    pub fn take_events() -> String {
        let events: Vec<serde_json::Value> = manager()
            .take_events()
            .into_iter()
            .map(|e| match e {
                Event::Finished(t) => serde_json::json!({ "kind": "finished", "id": t.id.to_string(), "title": t.display_name, "detail": t.final_path.map(|p| tilde(&p)) }),
                Event::Failed(t) => serde_json::json!({ "kind": "failed", "id": t.id.to_string(), "title": t.display_name, "detail": t.error }),
            })
            .collect();
        serde_json::to_string(&events).unwrap_or_default()
    }

    /// Changes one setting: `key` is a field of the snapshot's settings, `value` its new value as text.
    pub fn set_setting(key: String, value: String) -> Result<(), String> {
        manager().set_setting(&key, &value)
    }

    /// Where yt-dlp and FFmpeg are, as JSON `{ytdlp, ffmpeg}` (null when missing).
    pub fn tools() -> String {
        let s = manager().settings();
        let find = |n: &str, o: &str| beep_core::settings::find_tool(n, o).map(|p| tilde(&p));
        serde_json::json!({ "ytdlp": find("yt-dlp", &s.ytdlp_path), "ffmpeg": find("ffmpeg", &s.ffmpeg_path) }).to_string()
    }

    /// Shows the file (or folder) selected in Finder / File Explorer.
    pub async fn reveal(path: String) -> Result<(), String> {
        let p = std::path::PathBuf::from(&path);
        let target = if p.exists() { p } else { p.parent().map(|d| d.to_path_buf()).unwrap_or(p) };
        reveal_path(target).await
    }

    /// Opens the file in its default app.
    pub async fn open(path: String) -> Result<(), String> {
        open_path(path).await
    }

    /// Moves the file to the Trash.
    pub async fn trash(path: String) -> Result<(), String> {
        move_to_trash(path).await.map(|_| ())
    }

    /// Asks for a download folder; resolves with its path, or null when cancelled.
    pub async fn pick_folder() -> Result<Option<String>, String> {
        let picked = pick_paths(nsh_core::file_dialog::OpenDialog::folders().button_label("Use Folder")).await?;
        Ok(picked.and_then(|p| p.into_iter().next()).map(|p| p.display().to_string()))
    }

    /// Writes the queue to disk now (the service also saves on its own every two seconds).
    pub fn flush() {
        manager().flush();
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Task domain model (plan §20): a common envelope with an explicit lifecycle.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::util::redact;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    /// A direct HTTP/HTTPS file.
    Http,
    /// A platform video resolved from a post URL.
    Media,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Resolving,
    Probing,
    Downloading,
    Retrying,
    Merging,
    Verifying,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Resolving | Self::Probing | Self::Downloading | Self::Retrying | Self::Merging | Self::Verifying
        )
    }
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Waiting",
            Self::Resolving => "Resolving link",
            Self::Probing => "Connecting",
            Self::Downloading => "Downloading",
            Self::Retrying => "Retrying",
            Self::Merging => "Merging",
            Self::Verifying => "Verifying",
            Self::Paused => "Paused",
            Self::Completed => "Done",
            Self::Failed => "Failed",
            Self::Cancelled => "Stopped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    X,
    Threads,
    Instagram,
    Other,
    Web,
}

impl Platform {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Threads => "Threads",
            Self::Instagram => "Instagram",
            Self::Other => "Video",
            Self::Web => "Web",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    /// Inclusive end. `u64::MAX` marks an open-ended stream of unknown length.
    pub end: u64,
    pub received: u64,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end, received: 0 }
    }
    pub fn len(&self) -> u64 {
        if self.end == u64::MAX { u64::MAX } else { self.end + 1 - self.start }
    }
    pub fn cursor(&self) -> u64 {
        self.start + self.received
    }
    pub fn remaining(&self) -> u64 {
        self.len().saturating_sub(self.received)
    }
    pub fn is_done(&self) -> bool {
        self.received >= self.len()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub length: Option<u64>,
}

impl Validators {
    /// A strong validator usable in `If-Range` (weak ETags are not allowed there).
    pub fn if_range(&self) -> Option<&str> {
        match &self.etag {
            Some(e) if !e.starts_with("W/") => Some(e),
            _ => self.last_modified.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub platform: Platform,
    pub page_url: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub thumbnail_url: Option<String>,
    pub format_label: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    /// Set when the chosen rendition is fetched and merged by yt-dlp.
    pub ytdlp_format: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// The quality the user asked for when adding (overrides the global preference).
    #[serde(default)]
    pub quality: Option<crate::settings::Quality>,
}

impl MediaInfo {
    pub fn new(platform: Platform, page_url: impl Into<String>) -> Self {
        Self {
            platform,
            page_url: page_url.into(),
            title: None,
            author: None,
            thumbnail_url: None,
            format_label: None,
            width: None,
            height: None,
            duration: None,
            ytdlp_format: None,
            headers: Vec::new(),
            quality: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub at_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: Uuid,
    pub kind: TaskKind,
    /// What the user pasted (post URL for media, file URL for http).
    pub source: String,
    /// Where bytes come from (resolved media URL, or the final URL after redirects).
    pub fetch_url: Option<String>,
    pub display_name: String,
    pub file_name: Option<String>,
    pub directory: PathBuf,
    pub final_path: Option<PathBuf>,
    pub status: TaskStatus,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub completed_ms: Option<u64>,
    pub total_bytes: Option<u64>,
    pub received_bytes: u64,
    pub accepts_ranges: bool,
    pub segments: Vec<Segment>,
    pub validators: Validators,
    pub max_connections: usize,
    pub media: Option<MediaInfo>,
    pub error: Option<String>,
    pub sha256: Option<String>,
    pub log: Vec<LogEntry>,
    /// Runtime metrics, not persisted.
    #[serde(skip)]
    pub speed: f64,
    #[serde(skip)]
    pub connections: usize,
}

impl DownloadTask {
    pub fn new(kind: TaskKind, source: &str, directory: PathBuf, max_connections: usize, media: Option<MediaInfo>) -> Self {
        let now = crate::util::now_ms();
        let display_name = match &media {
            Some(m) => format!("{} post", m.platform.display_name()),
            None => crate::util::name_from_url(source).unwrap_or_else(|| "download".into()),
        };
        Self {
            id: Uuid::new_v4(),
            kind,
            source: source.to_owned(),
            fetch_url: None,
            display_name,
            file_name: None,
            directory,
            final_path: None,
            status: TaskStatus::Queued,
            created_ms: now,
            updated_ms: now,
            completed_ms: None,
            total_bytes: None,
            received_bytes: 0,
            accepts_ranges: false,
            segments: Vec::new(),
            validators: Validators::default(),
            max_connections,
            media,
            error: None,
            sha256: None,
            log: Vec::new(),
            speed: 0.0,
            connections: 0,
        }
    }

    pub fn progress(&self) -> f64 {
        match self.total_bytes {
            Some(t) if t > 0 => (self.received_bytes as f64 / t as f64).min(1.0),
            _ => 0.0,
        }
    }

    /// Seconds remaining at the smoothed speed.
    pub fn eta(&self) -> Option<f64> {
        let total = self.total_bytes?;
        (self.speed > 1.0).then(|| total.saturating_sub(self.received_bytes) as f64 / self.speed)
    }

    pub fn host(&self) -> String {
        let src = self.media.as_ref().map(|m| m.page_url.as_str()).unwrap_or(&self.source);
        url::Url::parse(src)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_owned()))
            .unwrap_or_default()
    }

    pub fn platform(&self) -> Platform {
        self.media.as_ref().map(|m| m.platform).unwrap_or(Platform::Web)
    }

    pub fn part_path(&self) -> Option<PathBuf> {
        self.file_name.as_ref().map(|f| self.directory.join(format!("{f}.beeppart")))
    }

    pub fn push_log(&mut self, message: impl AsRef<str>) {
        self.log.push(LogEntry { at_ms: crate::util::now_ms(), message: redact(message.as_ref()) });
        if self.log.len() > 200 {
            let extra = self.log.len() - 200;
            self.log.drain(..extra);
        }
    }
}

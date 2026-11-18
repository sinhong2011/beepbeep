// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    #[default]
    Best,
    P1080,
    P720,
    P480,
    Audio,
}

impl Quality {
    pub const ALL: [Quality; 5] = [Quality::Best, Quality::P1080, Quality::P720, Quality::P480, Quality::Audio];
    pub fn label(self) -> &'static str {
        match self {
            Quality::Best => "Best available",
            Quality::P1080 => "1080p",
            Quality::P720 => "720p",
            Quality::P480 => "480p",
            Quality::Audio => "Audio only",
        }
    }
    pub fn max_side(self) -> Option<u32> {
        match self {
            Quality::P1080 => Some(1080),
            Quality::P720 => Some(720),
            Quality::P480 => Some(480),
            _ => None,
        }
    }
    pub fn parse(s: &str) -> Option<Quality> {
        match s.to_ascii_lowercase().trim_end_matches('p') {
            "best" => Some(Quality::Best),
            "1080" => Some(Quality::P1080),
            "720" => Some(Quality::P720),
            "480" => Some(Quality::P480),
            "audio" => Some(Quality::Audio),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CookieBrowser {
    #[default]
    None,
    Safari,
    Chrome,
    Firefox,
    Edge,
    Brave,
}

impl CookieBrowser {
    pub const ALL: [CookieBrowser; 6] =
        [Self::None, Self::Safari, Self::Chrome, Self::Firefox, Self::Edge, Self::Brave];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Safari => "Safari",
            Self::Chrome => "Chrome",
            Self::Firefox => "Firefox",
            Self::Edge => "Edge",
            Self::Brave => "Brave",
        }
    }
    pub fn arg(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Safari => Some("safari"),
            Self::Chrome => Some("chrome"),
            Self::Firefox => Some("firefox"),
            Self::Edge => Some("edge"),
            Self::Brave => Some("brave"),
        }
    }
}

/// Settings. `#[serde(default)]` keeps old files loading when fields are added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub max_concurrent: usize,
    pub connections_per_task: usize,
    pub per_host_cap: usize,
    pub min_segment: u64,
    /// Bytes per second across all tasks; 0 = unlimited.
    pub speed_limit: u64,
    pub notify: bool,
    pub verify_sha256: bool,
    pub quality: Quality,
    pub prefer_native: bool,
    pub ytdlp_path: String,
    pub ffmpeg_path: String,
    pub cookie_browser: CookieBrowser,
    pub category_shortcuts: bool,
    pub user_agent: String,
}

pub const DEFAULT_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: crate::util::default_download_dir(),
            max_concurrent: 3,
            connections_per_task: 8,
            per_host_cap: 16,
            min_segment: 1 << 20,
            speed_limit: 0,
            notify: true,
            verify_sha256: false,
            quality: Quality::Best,
            prefer_native: true,
            ytdlp_path: String::new(),
            ffmpeg_path: String::new(),
            cookie_browser: CookieBrowser::None,
            category_shortcuts: true,
            user_agent: DEFAULT_UA.into(),
        }
    }
}

/// External tool discovery (plan §14).
pub fn find_tool(name: &str, override_path: &str) -> Option<PathBuf> {
    let expand = |p: &str| -> PathBuf {
        if let (Some(rest), Some(h)) = (p.strip_prefix("~/"), dirs::home_dir()) { h.join(rest) } else { PathBuf::from(p) }
    };
    let is_exec = |p: &PathBuf| p.is_file();
    if !override_path.is_empty() {
        let p = expand(override_path);
        if is_exec(&p) {
            return Some(p);
        }
    }
    let mut dirs_: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs_.push(PathBuf::from(d));
    }
    if let Some(h) = dirs::home_dir() {
        dirs_.push(h.join(".local/bin"));
    }
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    dirs_.into_iter().map(|d| d.join(&exe)).find(is_exec)
}

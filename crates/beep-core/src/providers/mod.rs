// SPDX-License-Identifier: AGPL-3.0-or-later
//! Video providers (plan §36): resolve a post URL into normalized `MediaItem`s with `MediaFormat`s.
//! Native resolvers run first; yt-dlp is the fallback (audit A3).

mod meta;
mod x;
mod ytdlp;

pub use meta::{InstagramProvider, MetaPage, ThreadsProvider, parse_meta_page};
pub use x::{XProvider, parse_syndication, x_token};
pub use ytdlp::{YtDlp, YtDlpProgress};

use crate::domain::Platform;
use crate::settings::{Quality, Settings, find_tool};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaFormat {
    pub id: String,
    /// Direct URL for the native engine; `None` means yt-dlp fetches (and merges) it.
    pub url: Option<String>,
    pub ytdlp_format: Option<String>,
    pub ext: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bitrate: Option<u64>,
    pub audio_only: bool,
    pub headers: Vec<(String, String)>,
}

impl MediaFormat {
    pub fn direct(id: impl Into<String>, url: impl Into<String>, width: Option<u32>, height: Option<u32>, bitrate: Option<u64>) -> Self {
        Self {
            id: id.into(),
            url: Some(url.into()),
            ytdlp_format: None,
            ext: "mp4".into(),
            width,
            height,
            bitrate,
            audio_only: false,
            headers: Vec::new(),
        }
    }

    /// The short side, so portrait videos rank like landscape ones (1080×1920 is "1080p").
    pub fn short_side(&self) -> u32 {
        match (self.width, self.height) {
            (Some(w), Some(h)) => w.min(h),
            (None, Some(h)) => h,
            (Some(w), None) => w,
            _ => 0,
        }
    }

    pub fn label(&self) -> String {
        if self.audio_only {
            return "Audio".into();
        }
        match (self.short_side(), self.bitrate) {
            (s, _) if s > 0 => format!("{s}p"),
            (_, Some(b)) => format!("{} kbps", b / 1000),
            _ => self.ext.to_uppercase(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaItem {
    pub id: String,
    pub platform: Platform,
    pub page_url: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub formats: Vec<MediaFormat>,
    pub resolved_by: &'static str,
}

impl MediaItem {
    /// The best format at or under the quality cap, preferring direct URLs (resumable, multi-connection).
    pub fn pick(&self, q: Quality) -> Option<&MediaFormat> {
        let rank = |f: &MediaFormat| (f.url.is_some() as u8, f.short_side(), f.bitrate.unwrap_or(0));
        if q == Quality::Audio {
            if let Some(a) = self.formats.iter().filter(|f| f.audio_only).max_by_key(|f| f.bitrate.unwrap_or(0)) {
                return Some(a);
            }
        }
        let video = || self.formats.iter().filter(|f| !f.audio_only);
        if let Some(cap) = q.max_side() {
            if let Some(f) = video().filter(|f| f.short_side() > 0 && f.short_side() <= cap).max_by_key(|f| rank(f)) {
                return Some(f);
            }
            return video().filter(|f| f.short_side() > 0).min_by_key(|f| f.short_side()).or_else(|| video().next());
        }
        video().max_by_key(|f| rank(f)).or(self.formats.first())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    Unsupported,
    NotFound,
    NoVideo,
    LoginRequired(Platform),
    RateLimited,
    ToolMissing,
    Failed(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "This link isn't a supported post."),
            Self::NotFound => write!(f, "The post wasn't found. It may be deleted or private."),
            Self::NoVideo => write!(f, "This post has no video."),
            Self::LoginRequired(p) => write!(
                f,
                "{} needs a login to show this post. Install yt-dlp and turn on browser cookies in Settings › Media.",
                p.display_name()
            ),
            Self::RateLimited => write!(f, "The platform is limiting requests. Wait a minute, then retry."),
            Self::ToolMissing => write!(f, "yt-dlp isn't installed. Install it with `brew install yt-dlp`, or set its path in Settings › Media."),
            Self::Failed(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for ResolveError {}

pub type ResolveFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<MediaItem>, ResolveError>> + Send + 'a>>;

pub trait VideoProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn can_handle(&self, url: &url::Url) -> bool;
    fn resolve<'a>(&'a self, url: &'a url::Url) -> ResolveFuture<'a>;
}

// ---------------------------------------------------------------------------------------------
// URL classification

const X_HOSTS: &[&str] = &[
    "x.com", "twitter.com", "mobile.twitter.com", "mobile.x.com", "fxtwitter.com", "vxtwitter.com", "fixupx.com", "fixvx.com",
];

fn host(u: &url::Url) -> String {
    u.host_str().unwrap_or("").to_ascii_lowercase().trim_start_matches("www.").to_owned()
}

fn segments(u: &url::Url) -> Vec<&str> {
    u.path_segments().map(|s| s.filter(|p| !p.is_empty()).collect()).unwrap_or_default()
}

pub fn tweet_id(u: &url::Url) -> Option<String> {
    let segs = segments(u);
    let i = segs.iter().position(|s| *s == "status" || *s == "statuses")?;
    let id = segs.get(i + 1)?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| id.to_string())
}

pub fn threads_code(u: &url::Url) -> Option<String> {
    let segs = segments(u);
    if let Some(i) = segs.iter().position(|s| *s == "post") {
        return segs.get(i + 1).map(|s| s.to_string());
    }
    (segs.first() == Some(&"t")).then(|| segs.get(1).map(|s| s.to_string())).flatten()
}

pub fn instagram_code(u: &url::Url) -> Option<String> {
    let segs = segments(u);
    let i = segs.iter().position(|s| matches!(*s, "p" | "reel" | "reels" | "tv"))?;
    segs.get(i + 1).map(|s| s.to_string())
}

/// Which platform a post URL belongs to; `None` for anything else (treated as a direct file).
pub fn classify(s: &str) -> Option<Platform> {
    let u = url::Url::parse(s).ok()?;
    let h = host(&u);
    if X_HOSTS.contains(&h.as_str()) && tweet_id(&u).is_some() {
        return Some(Platform::X);
    }
    if (h == "threads.net" || h == "threads.com") && threads_code(&u).is_some() {
        return Some(Platform::Threads);
    }
    if (h == "instagram.com" || h == "m.instagram.com" || h == "instagr.am") && instagram_code(&u).is_some() {
        return Some(Platform::Instagram);
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Resolver chain

pub struct ResolverChain {
    native: Vec<Box<dyn VideoProvider>>,
    pub ytdlp: Option<YtDlp>,
    prefer_native: bool,
}

impl ResolverChain {
    pub fn new(settings: &Settings) -> Self {
        let client = crate::engine::http_client(&settings.user_agent);
        let ytdlp = find_tool("yt-dlp", &settings.ytdlp_path).map(|p| YtDlp {
            path: p,
            ffmpeg: find_tool("ffmpeg", &settings.ffmpeg_path),
            cookies: settings.cookie_browser,
        });
        Self {
            native: vec![
                Box::new(XProvider { client: client.clone() }),
                Box::new(ThreadsProvider { client: client.clone() }),
                Box::new(InstagramProvider { client }),
            ],
            ytdlp,
            prefer_native: settings.prefer_native,
        }
    }

    pub async fn resolve(&self, s: &str, mut log: impl FnMut(String)) -> Result<Vec<MediaItem>, ResolveError> {
        let u = url::Url::parse(s).map_err(|_| ResolveError::Unsupported)?;
        let native = self.native.iter().find(|p| p.can_handle(&u)).map(|p| p.as_ref());
        let yt = self.ytdlp.as_ref().map(|y| y as &dyn VideoProvider);
        let order: Vec<&dyn VideoProvider> =
            if self.prefer_native { [native, yt].into_iter().flatten().collect() } else { [yt, native].into_iter().flatten().collect() };
        if order.is_empty() {
            return Err(ResolveError::ToolMissing);
        }
        let mut first: Option<ResolveError> = None;
        for p in order {
            match p.resolve(&u).await {
                Ok(items) => {
                    log(format!("Resolved with {}: {} video(s)", p.name(), items.len()));
                    return Ok(items);
                }
                Err(e) => {
                    log(format!("{}: {e}", p.name()));
                    // Keep the most useful error: a native "no video" is less useful than a yt-dlp detail.
                    if first.is_none() || first == Some(ResolveError::NoVideo) {
                        first = Some(e);
                    }
                }
            }
        }
        Err(first.unwrap_or(ResolveError::NoVideo))
    }
}

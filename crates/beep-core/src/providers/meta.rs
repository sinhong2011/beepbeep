// SPDX-License-Identifier: AGPL-3.0-or-later
//! Threads and Instagram: video URLs from the public post page (`video_versions`, `video_url`, `og:video`).
//! Posts that need a login fall through to yt-dlp (with browser cookies when the user turns them on).

use super::{MediaFormat, MediaItem, ResolveError, ResolveFuture, VideoProvider, instagram_code, threads_code};
use crate::domain::Platform;
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MetaPage {
    pub videos: Vec<MediaFormat>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub thumbnail: Option<String>,
}

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

pub fn parse_meta_page(html: &str) -> MetaPage {
    static VV: OnceLock<Regex> = OnceLock::new();
    static OBJ: OnceLock<Regex> = OnceLock::new();
    static URL: OnceLock<Regex> = OnceLock::new();
    static W: OnceLock<Regex> = OnceLock::new();
    static H: OnceLock<Regex> = OnceLock::new();
    static VURL: OnceLock<Regex> = OnceLock::new();
    static HANDLE: OnceLock<Regex> = OnceLock::new();

    let mut page = MetaPage::default();
    let mut seen = HashSet::new();
    let mut add = |raw: &str, w: Option<u32>, h: Option<u32>, out: &mut Vec<MediaFormat>| {
        let s = unescape_json(raw);
        let Ok(u) = url::Url::parse(&s) else { return };
        if u.scheme() != "https" || !seen.insert(u.path().to_owned()) {
            return; // renditions differ by path; query strings are per-request signatures
        }
        out.push(MediaFormat::direct(format!("v{}", out.len()), s, w, h, None));
    };

    // 1) "video_versions":[{"type":101,"width":720,"height":1280,"url":"…"}, …]
    let vv = re(&VV, r#"(?s)"video_versions"\s*:\s*\[(.*?)\]"#);
    let obj = re(&OBJ, r"\{[^{}]*\}");
    let url_re = re(&URL, r#""url"\s*:\s*"((?:[^"\\]|\\.)*)""#);
    let w_re = re(&W, r#""width"\s*:\s*(\d+)"#);
    let h_re = re(&H, r#""height"\s*:\s*(\d+)"#);
    for block in vv.captures_iter(html) {
        for o in obj.find_iter(&block[1]) {
            let o = o.as_str();
            let Some(u) = url_re.captures(o) else { continue };
            let w = w_re.captures(o).and_then(|c| c[1].parse().ok());
            let h = h_re.captures(o).and_then(|c| c[1].parse().ok());
            add(&u[1], w, h, &mut page.videos);
        }
    }
    // 2) legacy "video_url":"…"
    for c in re(&VURL, r#""video_url"\s*:\s*"((?:[^"\\]|\\.)*)""#).captures_iter(html) {
        add(&c[1], None, None, &mut page.videos);
    }
    // 3) OpenGraph
    let ogw = meta(html, "og:video:width").and_then(|v| v.parse().ok());
    let ogh = meta(html, "og:video:height").and_then(|v| v.parse().ok());
    for key in ["og:video:secure_url", "og:video", "og:video:url"] {
        if let Some(v) = meta(html, key) {
            add(&decode_entities(&v), ogw, ogh, &mut page.videos);
        }
    }

    let og_title = meta(html, "og:title").map(|t| decode_entities(&t));
    page.title = meta(html, "og:description").map(|t| decode_entities(&t)).or(og_title.clone());
    page.author = og_title.and_then(|t| {
        // "Name (@handle) on Threads" / "Name on Instagram: …"
        if let Some(c) = re(&HANDLE, r"\(@([A-Za-z0-9._]+)\)").captures(&t) {
            return Some(format!("@{}", &c[1]));
        }
        t.split(" on ").next().map(str::to_owned).filter(|s| !s.is_empty())
    });
    page.thumbnail = meta(html, "og:image").map(|t| decode_entities(&t));
    page
}

fn meta(html: &str, property: &str) -> Option<String> {
    let p = regex::escape(property);
    let a = Regex::new(&format!(r#"<meta[^>]+(?:property|name)=["']{p}["'][^>]*content=["']([^"']*)["']"#)).ok()?;
    let b = Regex::new(&format!(r#"<meta[^>]+content=["']([^"']*)["'][^>]*(?:property|name)=["']{p}["']"#)).ok()?;
    a.captures(html).or_else(|| b.captures(html)).map(|c| c[1].to_owned()).filter(|s| !s.is_empty())
}

fn unescape_json(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    serde_json::from_str::<String>(&format!("\"{s}\"")).unwrap_or_else(|_| s.replace("\\/", "/"))
}

fn decode_entities(s: &str) -> String {
    static NUM: OnceLock<Regex> = OnceLock::new();
    let s = s
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#x2F;", "/");
    let s = re(&NUM, r"&#x([0-9a-fA-F]+);|&#(\d+);")
        .replace_all(&s, |c: &regex::Captures| {
            let code = c.get(1).and_then(|h| u32::from_str_radix(h.as_str(), 16).ok()).or_else(|| c.get(2).and_then(|d| d.as_str().parse().ok()));
            code.and_then(char::from_u32).map(String::from).unwrap_or_default()
        })
        .into_owned();
    s.replace("&amp;", "&")
}

async fn fetch_html(client: &Client, url: &str) -> Result<String, ResolveError> {
    let resp = client
        .get(url)
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Sec-Fetch-Mode", "navigate")
        .send()
        .await
        .map_err(|e| ResolveError::Failed(crate::engine::EngineError::from(e).to_string()))?;
    match resp.status().as_u16() {
        200..=299 => resp.text().await.map_err(|e| ResolveError::Failed(e.to_string())),
        404 | 410 => Err(ResolveError::NotFound),
        429 => Err(ResolveError::RateLimited),
        c => Err(ResolveError::Failed(format!("HTTP {c}"))),
    }
}

pub struct ThreadsProvider {
    pub client: Client,
}

impl VideoProvider for ThreadsProvider {
    fn name(&self) -> &'static str {
        "threads-page"
    }
    fn can_handle(&self, url: &url::Url) -> bool {
        super::classify(url.as_str()) == Some(Platform::Threads)
    }
    fn resolve<'a>(&'a self, url: &'a url::Url) -> ResolveFuture<'a> {
        Box::pin(async move {
            let code = threads_code(url).ok_or(ResolveError::Unsupported)?;
            let html = fetch_html(&self.client, url.as_str()).await?;
            let page = parse_meta_page(&html);
            if page.videos.is_empty() {
                return Err(if page.title.is_none() { ResolveError::LoginRequired(Platform::Threads) } else { ResolveError::NoVideo });
            }
            Ok(vec![MediaItem {
                id: format!("threads-{code}"),
                platform: Platform::Threads,
                page_url: url.to_string(),
                title: page.title,
                author: page.author,
                thumbnail: page.thumbnail,
                duration: None,
                formats: page.videos,
                resolved_by: "threads-page",
            }])
        })
    }
}

pub struct InstagramProvider {
    pub client: Client,
}

impl VideoProvider for InstagramProvider {
    fn name(&self) -> &'static str {
        "instagram-page"
    }
    fn can_handle(&self, url: &url::Url) -> bool {
        super::classify(url.as_str()) == Some(Platform::Instagram)
    }
    fn resolve<'a>(&'a self, url: &'a url::Url) -> ResolveFuture<'a> {
        Box::pin(async move {
            let code = instagram_code(url).ok_or(ResolveError::Unsupported)?;
            let page_url = format!("https://www.instagram.com/p/{code}/");
            let mut best: Option<MetaPage> = None;
            // The post page first, then its public embed page.
            for candidate in [page_url.clone(), format!("https://www.instagram.com/p/{code}/embed/captioned/")] {
                let Ok(html) = fetch_html(&self.client, &candidate).await else { continue };
                let page = parse_meta_page(&html);
                let found = !page.videos.is_empty();
                if best.is_none() || found {
                    best = Some(page);
                }
                if found {
                    break;
                }
            }
            let page = best.filter(|p| !p.videos.is_empty()).ok_or(ResolveError::LoginRequired(Platform::Instagram))?;
            Ok(vec![MediaItem {
                id: format!("ig-{code}"),
                platform: Platform::Instagram,
                page_url,
                title: page.title,
                author: page.author,
                thumbnail: page.thumbnail,
                duration: None,
                formats: page.videos,
                resolved_by: "instagram-page",
            }])
        })
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! X / Twitter: the public embed (syndication) JSON that powers embedded posts. No account needed.

use super::{MediaFormat, MediaItem, ResolveError, ResolveFuture, VideoProvider, tweet_id};
use crate::domain::Platform;
use reqwest::Client;
use serde_json::Value;

pub struct XProvider {
    pub client: Client,
}

impl VideoProvider for XProvider {
    fn name(&self) -> &'static str {
        "x-embed"
    }
    fn can_handle(&self, url: &url::Url) -> bool {
        super::classify(url.as_str()) == Some(Platform::X)
    }
    fn resolve<'a>(&'a self, url: &'a url::Url) -> ResolveFuture<'a> {
        Box::pin(async move {
            let id = tweet_id(url).ok_or(ResolveError::Unsupported)?;
            let resp = self
                .client
                .get("https://cdn.syndication.twimg.com/tweet-result")
                .query(&[("id", id.as_str()), ("token", &x_token(&id)), ("lang", "en")])
                .send()
                .await
                .map_err(|e| ResolveError::Failed(crate::engine::EngineError::from(e).to_string()))?;
            match resp.status().as_u16() {
                200 => {}
                404 => return Err(ResolveError::NotFound),
                429 => return Err(ResolveError::RateLimited),
                c => return Err(ResolveError::Failed(format!("X answered HTTP {c}"))),
            }
            let body = resp.text().await.map_err(|e| ResolveError::Failed(e.to_string()))?;
            let json: Value = serde_json::from_str(&body).map_err(|_| ResolveError::NotFound)?;
            if json.as_object().is_none_or(|o| o.is_empty()) {
                return Err(ResolveError::NotFound);
            }
            if json["__typename"] == "TweetTombstone" {
                return Err(ResolveError::LoginRequired(Platform::X));
            }
            parse_syndication(&json, &format!("https://x.com/i/status/{id}"), &id)
        })
    }
}

/// The token the embed widget sends: `((id / 1e15) * π).toString(36)` with zeros and the dot removed.
pub fn x_token(id: &str) -> String {
    let n = id.parse::<f64>().unwrap_or(0.0) / 1e15 * std::f64::consts::PI;
    js_to_string_36(n).chars().filter(|c| *c != '0' && *c != '.').collect()
}

/// Port of V8's `DoubleToRadixCString` for positive doubles: the shortest digits that round-trip,
/// exactly what JavaScript's `Number.prototype.toString(36)` prints.
fn js_to_string_36(value: f64) -> String {
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let radix = 36.0;
    let mut integer = value.floor();
    let mut fraction = value - integer;
    let next_up = f64::from_bits(value.to_bits() + 1);
    let mut delta = (0.5 * (next_up - value)).max(f64::from_bits(1));
    let mut frac: Vec<usize> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= radix;
            delta *= radix;
            let digit = fraction as usize;
            frac.push(digit);
            fraction -= digit as f64;
            if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
                // Round up, carrying backwards.
                loop {
                    match frac.pop() {
                        None => {
                            integer += 1.0;
                            break;
                        }
                        Some(d) if d + 1 < 36 => {
                            frac.push(d + 1);
                            break;
                        }
                        Some(_) => {}
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    let mut int_digits = Vec::new();
    if integer == 0.0 {
        int_digits.push(b'0');
    }
    while integer > 0.0 {
        int_digits.push(CHARS[(integer % radix) as usize]);
        integer = (integer / radix).floor();
    }
    int_digits.reverse();
    let mut s = String::from_utf8(int_digits).unwrap();
    if !frac.is_empty() {
        s.push('.');
        s.extend(frac.iter().map(|d| CHARS[*d] as char));
    }
    s
}

pub fn parse_syndication(json: &Value, page_url: &str, id: &str) -> Result<Vec<MediaItem>, ResolveError> {
    let handle = json["user"]["screen_name"].as_str();
    let text = json["text"].as_str().map(clean_text).filter(|t| !t.is_empty());
    let mut media = json["mediaDetails"].as_array().cloned().unwrap_or_default();
    if media.is_empty() {
        media = json["quoted_tweet"]["mediaDetails"].as_array().cloned().unwrap_or_default();
    }
    let count = media.iter().filter(|m| matches!(m["type"].as_str(), Some("video" | "animated_gif"))).count();
    let mut items = Vec::new();
    for (n, m) in media.iter().filter(|m| matches!(m["type"].as_str(), Some("video" | "animated_gif"))).enumerate() {
        let info = &m["video_info"];
        let formats: Vec<MediaFormat> = info["variants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["content_type"] == "video/mp4")
            .filter_map(|v| {
                let url = v["url"].as_str()?;
                let (w, h) = dims_from_url(url);
                let br = v["bitrate"].as_u64();
                Some(MediaFormat::direct(format!("mp4-{}-{}", br.unwrap_or(0), h.unwrap_or(0)), url, w, h, br))
            })
            .collect();
        if formats.is_empty() {
            continue;
        }
        let suffix = if count > 1 { format!(" ({})", n + 1) } else { String::new() };
        items.push(MediaItem {
            id: format!("{id}-{n}"),
            platform: Platform::X,
            page_url: page_url.to_owned(),
            title: Some(text.clone().unwrap_or_else(|| format!("Post by @{}", handle.unwrap_or("x"))) + &suffix),
            author: handle.map(|h| format!("@{h}")),
            thumbnail: m["media_url_https"].as_str().map(str::to_owned),
            duration: info["duration_millis"].as_f64().map(|ms| ms / 1000.0),
            formats,
            resolved_by: "x-embed",
        });
    }
    if items.is_empty() { Err(ResolveError::NoVideo) } else { Ok(items) }
}

fn clean_text(s: &str) -> String {
    s.split_whitespace().filter(|w| !w.starts_with("https://t.co/")).collect::<Vec<_>>().join(" ")
}

/// `…/vid/1280x720/…mp4` → (1280, 720)
fn dims_from_url(s: &str) -> (Option<u32>, Option<u32>) {
    for part in s.split('/') {
        if let Some((w, h)) = part.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                return (Some(w), Some(h));
            }
        }
    }
    (None, None)
}

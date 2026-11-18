// SPDX-License-Identifier: AGPL-3.0-or-later
//! Formatting, redaction, link collection and file safety helpers.

use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Redaction (plan §11, §43): signed query parameters never reach logs.

const SENSITIVE: &[&str] = &[
    "token", "sig", "signature", "oh", "oe", "_nc_sid", "_nc_ohc", "_nc_ht", "efg", "expires", "x-amz-signature",
    "x-amz-credential", "x-amz-security-token", "auth", "access_token", "key", "policy", "hmac", "ccb", "edm",
];

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"https?://[^\s"'<>]+"#).unwrap())
}

pub fn redact_url(s: &str) -> String {
    let Ok(mut u) = url::Url::parse(s) else { return s.to_owned() };
    if u.query().is_none() {
        return s.to_owned();
    }
    let pairs: Vec<(String, String)> = u
        .query_pairs()
        .map(|(k, v)| {
            let hide = SENSITIVE.contains(&k.to_ascii_lowercase().as_str());
            (k.into_owned(), if hide { "•••".to_owned() } else { v.into_owned() })
        })
        .collect();
    u.query_pairs_mut().clear().extend_pairs(pairs);
    u.to_string()
}

pub fn redact(s: &str) -> String {
    if !s.contains("http") {
        return s.to_owned();
    }
    url_re().replace_all(s, |c: &regex::Captures| redact_url(&c[0])).into_owned()
}

// ---------------------------------------------------------------------------------------------
// Formatting (tabular, short).

pub fn fmt_bytes(b: Option<u64>) -> String {
    let Some(b) = b else { return "—".into() };
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else if v >= 100.0 { format!("{v:.0} {}", U[i]) } else { format!("{v:.1} {}", U[i]) }
}

pub fn fmt_speed(bps: f64) -> String {
    if bps < 1.0 { "—".into() } else { format!("{}/s", fmt_bytes(Some(bps as u64))) }
}

pub fn fmt_eta(secs: Option<f64>) -> String {
    let Some(t) = secs.filter(|t| t.is_finite() && *t >= 0.0) else { return "—".into() };
    let s = t.round() as u64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

pub fn fmt_duration(d: Option<f64>) -> Option<String> {
    let s = d.filter(|d| *d > 0.0)?.round() as u64;
    Some(if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) })
}

// ---------------------------------------------------------------------------------------------
// Link collector (plan §13).

/// Every distinct http(s) URL in arbitrary pasted text, in order.
pub fn extract_links(text: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"https?://[^\s<>"'`{}\[\]|\\^]+"#).unwrap());
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for m in re.find_iter(text) {
        let mut s = m.as_str();
        // Trailing punctuation from prose, and an unbalanced closing paren.
        loop {
            let trimmed = s.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"']);
            let trimmed = if trimmed.ends_with(')') && trimmed.matches('(').count() < trimmed.matches(')').count() {
                &trimmed[..trimmed.len() - 1]
            } else {
                trimmed
            };
            if trimmed.len() == s.len() {
                break;
            }
            s = trimmed;
        }
        if url::Url::parse(s).is_ok() && seen.insert(s.to_owned()) {
            out.push(s.to_owned());
        }
    }
    out
}

pub fn name_from_url(s: &str) -> Option<String> {
    let u = url::Url::parse(s).ok()?;
    let last = u.path_segments()?.filter(|p| !p.is_empty()).last()?;
    let decoded = percent_encoding::percent_decode_str(last).decode_utf8_lossy().into_owned();
    (!decoded.is_empty()).then_some(decoded)
}

// ---------------------------------------------------------------------------------------------
// File safety (plan §24).

/// A filename safe on macOS, Windows and Linux.
pub fn sanitize(name: &str, max_len: usize) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let mut s = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    s = s.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_owned();
    if s.is_empty() {
        s = "download".into();
    }
    if s.chars().count() > max_len {
        let (base, ext) = split_ext(&s);
        let keep = max_len.saturating_sub(ext.as_ref().map_or(0, |e| e.chars().count() + 1)).max(1);
        let base: String = base.chars().take(keep).collect();
        s = match ext {
            Some(e) => format!("{}.{e}", base.trim_end()),
            None => base,
        };
    }
    s
}

pub fn split_ext(name: &str) -> (String, Option<String>) {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 && name.len() - i <= 8 => (name[..i].to_owned(), Some(name[i + 1..].to_owned())),
        _ => (name.to_owned(), None),
    }
}

/// A path in `dir` that does not exist yet, appending " (n)" before the extension.
/// `reserved` holds names other tasks have claimed; partial files count unless `check_partials` is false.
pub fn unique_path(dir: &Path, name: &str, reserved: &HashSet<String>, check_partials: bool) -> PathBuf {
    let (base, ext) = split_ext(name);
    let mut n = 0;
    loop {
        let candidate = match (n, &ext) {
            (0, _) => name.to_owned(),
            (n, Some(e)) => format!("{base} ({n}).{e}"),
            (n, None) => format!("{base} ({n})"),
        };
        let p = dir.join(&candidate);
        let part = dir.join(format!("{candidate}.beeppart"));
        if !p.exists() && !(check_partials && part.exists()) && !reserved.contains(&candidate) {
            return p;
        }
        n += 1;
    }
}

/// Free bytes on the volume holding `dir` (or its nearest existing parent): macOS, Linux and Windows.
pub fn free_space(dir: &Path) -> Option<u64> {
    let existing = dir.ancestors().find(|p| p.exists())?;
    fs4::available_space(existing).ok()
}

/// Default download folder: ~/Downloads/BeepBeep.
pub fn default_download_dir() -> PathBuf {
    dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("BeepBeep")
}

/// Where the queue and settings live: `BEEPBEEP_DATA_DIR`, else the platform's data folder.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("BEEPBEEP_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("BeepBeep")
}

pub fn tilde(p: &Path) -> String {
    match dirs::home_dir() {
        Some(h) if p.starts_with(&h) => format!("~/{}", p.strip_prefix(&h).unwrap().display()),
        _ => p.display().to_string(),
    }
}

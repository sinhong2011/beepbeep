// SPDX-License-Identifier: AGPL-3.0-or-later
//! `beep mcp`: a Model Context Protocol server over stdio (plan §27), so AI assistants can queue,
//! inspect and control downloads. JSON-RPC 2.0, one message per line. Tools that destroy data are
//! annotated `destructiveHint` so clients ask the user first; nothing here bypasses access controls.

use beep_core::providers::ResolverChain;
use beep_core::util::{fmt_bytes, fmt_eta, fmt_speed};
use beep_core::{DownloadTask, Quality, Service, TaskStatus};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";

pub fn serve(m: Service) -> i32 {
    let m = &m;
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(m, &rt, &msg),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
    m.flush();
    0
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn handle(m: &Service, rt: &tokio::runtime::Runtime, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str().unwrap_or("");
    // Notifications (no id) get no reply.
    let id = id?;
    Some(match method {
        "initialize" => ok(
            id,
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "beepbeep", "title": "BeepBeep downloads", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Queue and manage downloads. add_download accepts direct file URLs and post links on X, Threads and Instagram (their video is saved). Use list_downloads or get_download to follow progress; downloads continue in the background. Only download content the user has the right to; remove_download with delete_file moves the file to the Trash and must be confirmed by the user."
            }),
        ),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"]["arguments"].clone();
            match call(m, rt, name, &args) {
                Ok(v) => ok(id, json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }], "structuredContent": v, "isError": false })),
                Err(e) => ok(id, json!({ "content": [{ "type": "text", "text": e }], "isError": true })),
            }
        }
        _ => error(id, -32601, &format!("method not found: {method}")),
    })
}

fn tools() -> Value {
    let id_arg = json!({ "type": "object", "properties": { "id": { "type": "string", "description": "The download's id, from add_download or list_downloads." } }, "required": ["id"] });
    json!([
        {
            "name": "add_download",
            "title": "Add downloads",
            "description": "Queue one or more downloads. Accepts direct http(s) file URLs and post links on X (x.com, twitter.com), Threads (threads.net/.com) and Instagram (/p/, /reel/); for posts the video is downloaded. Returns the new downloads.",
            "inputSchema": { "type": "object", "properties": {
                "urls": { "type": "array", "items": { "type": "string" }, "description": "Links to download." },
                "quality": { "type": "string", "enum": ["best", "1080", "720", "480", "audio"], "description": "Video quality for posts (default: the user's setting)." }
            }, "required": ["urls"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": true }
        },
        {
            "name": "list_downloads",
            "title": "List downloads",
            "description": "The downloads with status, progress, speed and file path, newest first.",
            "inputSchema": { "type": "object", "properties": {
                "filter": { "type": "string", "enum": ["all", "active", "completed", "failed"], "description": "Which downloads (default all)." },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "At most this many (default 50)." }
            } },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_download",
            "title": "Get a download",
            "description": "One download in detail: status, progress, size, file path, error, media info and its recent activity log.",
            "inputSchema": id_arg,
            "annotations": { "readOnlyHint": true }
        },
        { "name": "pause_download", "title": "Pause a download", "description": "Pauses a download; resume_download continues it where it stopped.", "inputSchema": id_arg, "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true } },
        { "name": "resume_download", "title": "Resume a download", "description": "Resumes a paused, failed or stopped download.", "inputSchema": id_arg, "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true } },
        {
            "name": "remove_download",
            "title": "Remove a download",
            "description": "Removes a download from the list. With delete_file, also moves the downloaded file to the Trash. Ask the user before deleting files.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "string" },
                "delete_file": { "type": "boolean", "description": "Also move the downloaded file to the Trash (default false)." }
            }, "required": ["id"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": true }
        },
        {
            "name": "resolve_link",
            "title": "Inspect a post link",
            "description": "Finds the videos in an X, Threads or Instagram post and their available qualities, without downloading.",
            "inputSchema": { "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] },
            "annotations": { "readOnlyHint": true, "openWorldHint": true }
        },
        {
            "name": "get_status",
            "title": "Download status",
            "description": "Overall throughput, how many downloads run and wait, and the download folder.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        }
    ])
}

/// A download as agents see it: plain fields, ready-formatted sizes.
pub fn summary(t: &DownloadTask, detail: bool) -> Value {
    let mut v = json!({
        "id": t.id.to_string(),
        "title": t.display_name,
        "status": format!("{:?}", t.status).to_lowercase(),
        "source": t.source,
        "progress_percent": t.total_bytes.map(|_| (t.progress() * 1000.0).round() / 10.0),
        "size": fmt_bytes(t.total_bytes),
        "downloaded": fmt_bytes(Some(t.received_bytes)),
        "speed": if t.status.is_active() { Some(fmt_speed(t.speed)) } else { None },
        "eta": if t.status == TaskStatus::Downloading { Some(fmt_eta(t.eta())) } else { None },
        "file": t.final_path.as_ref().map(|p| p.display().to_string()),
        "error": t.error,
    });
    if let Some(m) = &t.media {
        v["media"] = json!({ "platform": m.platform.display_name(), "author": m.author, "quality": m.format_label, "thumbnail": m.thumbnail_url });
    }
    if detail {
        v["folder"] = json!(t.directory.display().to_string());
        v["resumable"] = json!(t.accepts_ranges);
        v["connections"] = json!(t.connections);
        v["sha256"] = json!(t.sha256);
        v["log"] = json!(t.log.iter().rev().take(20).map(|l| l.message.clone()).collect::<Vec<_>>());
    }
    v
}

fn find(m: &Service, id: &str) -> Result<DownloadTask, String> {
    m.snapshot().tasks.into_iter().find(|t| t.id.to_string() == id).ok_or_else(|| format!("no download with id {id}"))
}

fn call(m: &Service, rt: &tokio::runtime::Runtime, name: &str, a: &Value) -> Result<Value, String> {
    let id = || a["id"].as_str().ok_or("missing id").map(str::to_owned);
    match name {
        "add_download" => {
            let urls: Vec<String> = a["urls"].as_array().ok_or("urls must be a list")?.iter().filter_map(|u| u.as_str().map(str::to_owned)).collect();
            let links: Vec<String> = urls.iter().flat_map(|u| beep_core::util::extract_links(u)).collect();
            if links.is_empty() {
                return Err("no http(s) links given".into());
            }
            let q = a["quality"].as_str().and_then(Quality::parse);
            let ids = m.add_urls(&links, q);
            let snap = m.snapshot();
            Ok(json!({ "added": ids.iter().filter_map(|i| snap.tasks.iter().find(|t| t.id == *i)).map(|t| summary(t, false)).collect::<Vec<_>>() }))
        }
        "list_downloads" => {
            let f = a["filter"].as_str().unwrap_or("all");
            let limit = a["limit"].as_u64().unwrap_or(50) as usize;
            let tasks: Vec<Value> = m
                .snapshot()
                .tasks
                .iter()
                .filter(|t| match f {
                    "active" => t.status.is_active() || matches!(t.status, TaskStatus::Queued | TaskStatus::Paused),
                    "completed" => t.status == TaskStatus::Completed,
                    "failed" => matches!(t.status, TaskStatus::Failed | TaskStatus::Cancelled),
                    _ => true,
                })
                .take(limit)
                .map(|t| summary(t, false))
                .collect();
            Ok(json!({ "downloads": tasks }))
        }
        "get_download" => Ok(summary(&find(m, &id()?)?, true)),
        "pause_download" => {
            let t = find(m, &id()?)?;
            m.pause(t.id);
            Ok(summary(&find(m, &t.id.to_string())?, false))
        }
        "resume_download" => {
            let t = find(m, &id()?)?;
            m.resume(t.id);
            Ok(summary(&find(m, &t.id.to_string())?, false))
        }
        "remove_download" => {
            let t = find(m, &id()?)?;
            let delete = a["delete_file"].as_bool().unwrap_or(false);
            let path = m.remove(t.id, delete);
            let mut trashed = None;
            if let Some(p) = path {
                trashed = Some(trash(&p).map(|_| p.display().to_string())?);
            }
            Ok(json!({ "removed": t.id.to_string(), "trashed_file": trashed }))
        }
        "resolve_link" => {
            let url = a["url"].as_str().ok_or("missing url")?;
            let chain = ResolverChain::new(&m.settings());
            let items = rt.block_on(chain.resolve(url, |_| {})).map_err(|e| e.to_string())?;
            Ok(json!({ "videos": items.iter().map(|i| json!({
                "title": i.title, "author": i.author, "platform": i.platform.display_name(), "thumbnail": i.thumbnail,
                "duration_seconds": i.duration, "resolved_by": i.resolved_by,
                "qualities": i.formats.iter().map(|f| f.label()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>() }))
        }
        "get_status" => {
            let snap = m.snapshot();
            let speed: f64 = snap.tasks.iter().filter(|t| t.status.is_active()).map(|t| t.speed).sum();
            Ok(json!({
                "speed": fmt_speed(speed),
                "running": snap.tasks.iter().filter(|t| t.status.is_active()).count(),
                "waiting": snap.tasks.iter().filter(|t| t.status == TaskStatus::Queued).count(),
                "paused": snap.tasks.iter().filter(|t| t.status == TaskStatus::Paused).count(),
                "completed": snap.tasks.iter().filter(|t| t.status == TaskStatus::Completed).count(),
                "failed": snap.tasks.iter().filter(|t| t.status == TaskStatus::Failed).count(),
                "folder": snap.settings.download_dir.display().to_string(),
            }))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

/// Moves a file to the Trash (macOS Finder via osascript; elsewhere it is deleted only on request).
fn trash(p: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let script = format!("tell application \"Finder\" to delete POSIX file \"{}\"", p.display().to_string().replace('"', "\\\""));
        let ok = std::process::Command::new("osascript").args(["-e", &script]).output().map(|o| o.status.success()).unwrap_or(false);
        if ok { Ok(()) } else { Err(format!("couldn't move {} to the Trash", p.display())) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::remove_file(p).map_err(|e| e.to_string())
    }
}

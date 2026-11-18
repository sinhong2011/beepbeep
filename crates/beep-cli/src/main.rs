// SPDX-License-Identifier: AGPL-3.0-or-later
//! `beep`: the headless CLI over the same service as the desktop app (plan §27), and `beep mcp`,
//! an MCP server for AI assistants. Every command takes `--json` for scripts and agents.

mod mcp;

use beep_core::providers::ResolverChain;
use beep_core::util::{fmt_bytes, fmt_duration, fmt_speed, redact_url};
use beep_core::{Manager, Quality, Role, Service, Settings, Store, TaskStatus};
use std::path::PathBuf;
use std::time::Duration;

const USAGE: &str = "beep - BeepBeep download manager

USAGE
  beep get <url>... [-o DIR] [-c CONNECTIONS] [-q best|1080|720|480|audio] [--limit BYTES_PER_SEC]
                    [--sha256] [--state DIR] [--json]
  beep list [--state DIR] [--json]   Show the queue kept in DIR
  beep resolve <url>... [--json]     Show the videos and formats of X, Threads and Instagram posts
  beep probe <url> [--json]          Show size, range support and validators
  beep mcp [--state DIR [-o DIR]]    Serve the queue to AI assistants over MCP (stdio); shares the
                                     app's queue unless --state gives a private one

With --json, progress goes to stderr and one JSON document to stdout.

Post links on x.com, twitter.com, threads.net/.com and instagram.com download their videos.
--state DIR keeps the queue on disk, so an interrupted `get` resumes when run again.";

struct Args(Vec<String>);
impl Args {
    fn opt(&mut self, name: &str) -> Option<String> {
        let i = self.0.iter().position(|a| a == name)?;
        let v = self.0.get(i + 1).cloned();
        self.0.drain(i..(i + 2).min(self.0.len()));
        v
    }
    fn flag(&mut self, name: &str) -> bool {
        let i = self.0.iter().position(|a| a == name);
        if let Some(i) = i {
            self.0.remove(i);
        }
        i.is_some()
    }
}

fn main() {
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() {
        println!("{USAGE}");
        return;
    }
    let cmd = raw.remove(0);
    let mut a = Args(raw);
    let json = a.flag("--json");
    let code = match cmd.as_str() {
        "get" => get(&mut a, json),
        "list" => list(&mut a, json),
        "resolve" => resolve(&a.0, json),
        "probe" => probe(&a.0, json),
        "mcp" => {
            // The user's one queue, shared with the desktop app (attached when it runs, owned otherwise);
            // `--state DIR` gives a private queue instead.
            // `-o` sets the folder of a private queue only: the shared queue keeps the user's own setting.
            let folder = a.opt("-o");
            let service = match a.opt("--state") {
                Some(dir) => {
                    let s = Service::private(Manager::new(Store::at(dir), None));
                    if let Some(f) = &folder {
                        let _ = s.set_setting("folder", f);
                    }
                    s
                }
                None => {
                    if folder.is_some() {
                        eprintln!("beep mcp: -o applies only with --state; the shared queue uses BeepBeep's download folder");
                    }
                    Service::connect_or_start(beep_core::util::data_dir(), None)
                }
            };
            eprintln!("beep mcp: {}", match service.role() { Role::Owner => "serving the queue", Role::Client => "attached to the running BeepBeep" });
            mcp::serve(service)
        }
        _ => {
            println!("{USAGE}");
            0
        }
    };
    std::process::exit(code);
}

fn list(a: &mut Args, json: bool) -> i32 {
    // The live queue (the running app's, if any); `--state DIR` reads a saved one without running it.
    let tasks = match a.opt("--state") {
        Some(dir) => Store::at(dir).load::<Vec<beep_core::DownloadTask>>("tasks.json").unwrap_or_default(),
        // Attach to a running owner; with none, read the saved queue (listing never starts downloads).
        None => match Service::connect(&beep_core::util::data_dir()) {
            Some(s) => s.snapshot().tasks,
            None => beep_core::default_store().load::<Vec<beep_core::DownloadTask>>("tasks.json").unwrap_or_default(),
        },
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&tasks.iter().map(|t| mcp::summary(t, false)).collect::<Vec<_>>()).unwrap());
    } else {
        for t in &tasks {
            println!("{:<12} {:>6} {:>10}  {}", t.status.label(), t.total_bytes.map(|_| format!("{:.0}%", t.progress() * 100.0)).unwrap_or_default(), fmt_bytes(t.total_bytes), t.display_name);
        }
    }
    0
}

fn get(a: &mut Args, json: bool) -> i32 {
    let mut s = Settings::default();
    s.download_dir = a.opt("-o").map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap());
    if let Some(c) = a.opt("-c").and_then(|c| c.parse().ok()) {
        s.connections_per_task = c;
    }
    if let Some(l) = a.opt("--limit").and_then(|l| l.parse().ok()) {
        s.speed_limit = l;
    }
    if let Some(q) = a.opt("-q").and_then(|q| Quality::parse(&q)) {
        s.quality = q;
    }
    s.verify_sha256 = a.flag("--sha256");
    s.max_concurrent = 4;
    let store = a.opt("--state").map(Store::at).unwrap_or_else(Store::memory);
    let urls: Vec<String> = a.0.iter().filter(|u| u.starts_with("http")).cloned().collect();

    let m = Manager::new(store, Some(s));
    let known: Vec<String> = m.snapshot().tasks.iter().map(|t| t.source.clone()).collect();
    let new: Vec<String> = urls.into_iter().filter(|u| !known.contains(u)).collect();
    if new.is_empty() && known.is_empty() {
        eprintln!("No URLs given.\n\n{USAGE}");
        return 2;
    }
    m.add_urls(&new, None);
    // `get` means "finish these": tasks an earlier interrupted run paused carry on.
    m.resume_all();

    let interrupted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let flag = interrupted.clone();
        let _ = ctrlc_hook(move || flag.store(true, std::sync::atomic::Ordering::SeqCst));
    }
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if interrupted.load(std::sync::atomic::Ordering::SeqCst) {
            m.pause_all();
            m.flush();
            println!("\ninterrupted; progress saved");
            return 130;
        }
        let snap = m.snapshot();
        let line = snap
            .tasks
            .iter()
            .map(|t| {
                let pct = if t.total_bytes.is_some() { format!("{:5.1}%", t.progress() * 100.0) } else { "  ?   ".into() };
                format!("{:<14} {pct} {:>11} x{:<2} {}", t.status.label(), fmt_speed(t.speed), t.connections, t.display_name.chars().take(50).collect::<String>())
            })
            .collect::<Vec<_>>()
            .join("\n");
        if line != last {
            if json { eprintln!("{line}\n---") } else { println!("{line}\n---") }
            last = line;
        }
        if !snap.tasks.is_empty() && snap.tasks.iter().all(|t| t.status.is_terminal()) {
            break;
        }
    }
    m.flush();
    let snap = m.snapshot().tasks;
    let failed_any = snap.iter().any(|t| t.status != TaskStatus::Completed);
    if json {
        println!("{}", serde_json::to_string_pretty(&snap.iter().map(|t| mcp::summary(t, true)).collect::<Vec<_>>()).unwrap());
        return if failed_any { 1 } else { 0 };
    }
    let mut failed = false;
    for t in snap {
        if t.status == TaskStatus::Completed {
            let p = t.final_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            let sha = t.sha256.as_ref().map(|s| format!("  sha256 {s}")).unwrap_or_default();
            println!("✓ {p}  {}{sha}", fmt_bytes(t.total_bytes));
        } else {
            failed = true;
            println!("✗ {}: {}", t.display_name, t.error.as_deref().unwrap_or(t.status.label()));
            for l in t.log.iter().rev().take(6).rev() {
                println!("    {}", l.message);
            }
        }
    }
    if failed { 1 } else { 0 }
}

/// SIGINT without an extra dependency: a thread waiting on a signal-backed pipe is overkill here,
/// so use tokio's signal support on a tiny runtime.
fn ctrlc_hook(f: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    std::thread::spawn(move || {
        rt.block_on(async {
            if tokio::signal::ctrl_c().await.is_ok() {
                f();
            }
        })
    });
    Ok(())
}

fn resolve(urls: &[String], json: bool) -> i32 {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let chain = ResolverChain::new(&Settings::default());
    if json {
        let out: Vec<serde_json::Value> = urls
            .iter()
            .map(|u| match rt.block_on(chain.resolve(u, |_| {})) {
                Ok(items) => serde_json::json!({ "url": u, "videos": items.iter().map(|i| serde_json::json!({
                    "title": i.title, "author": i.author, "platform": i.platform.display_name(), "thumbnail": i.thumbnail,
                    "duration_seconds": i.duration, "resolved_by": i.resolved_by,
                    "formats": i.formats.iter().map(|f| serde_json::json!({ "label": f.label(), "ext": f.ext, "direct": f.url.is_some() })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>() }),
                Err(e) => serde_json::json!({ "url": u, "error": e.to_string() }),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return if out.iter().any(|o| o.get("error").is_some()) { 1 } else { 0 };
    }
    println!("yt-dlp: {}", chain.ytdlp.as_ref().map(|y| y.path.display().to_string()).unwrap_or("not found".into()));
    let mut code = 0;
    for u in urls {
        println!("\n{u}  [{}]", beep_core::providers::classify(u).map_or("not a post link", |p| p.display_name()));
        match rt.block_on(chain.resolve(u, |l| println!("  · {l}"))) {
            Ok(items) => {
                for i in items {
                    println!(
                        "  {}  by {}  {}",
                        i.title.as_deref().unwrap_or("untitled").chars().take(70).collect::<String>(),
                        i.author.as_deref().unwrap_or("?"),
                        fmt_duration(i.duration).unwrap_or_default()
                    );
                    for f in &i.formats {
                        let target = f.url.as_deref().map(redact_url).or(f.ytdlp_format.clone()).unwrap_or_default();
                        println!("    {:<8} {:<4} {}", f.label(), f.ext, target.chars().take(100).collect::<String>());
                    }
                    if let Some(p) = i.pick(Quality::Best) {
                        println!("    → best: {}", p.label());
                    }
                }
            }
            Err(e) => {
                println!("  ✗ {e}");
                code = 1;
            }
        }
    }
    code
}

fn probe(urls: &[String], json: bool) -> i32 {
    let Some(u) = urls.first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = beep_core::engine::http_client(beep_core::settings::DEFAULT_UA);
    match rt.block_on(beep_core::engine::probe(&client, u, &[])) {
        Ok(p) if json => {
            println!("{}", serde_json::json!({ "url": p.final_url, "size": p.length, "ranges": p.accepts_ranges, "etag": p.validators.etag, "last_modified": p.validators.last_modified, "file_name": p.file_name, "mime": p.mime }));
            0
        }
        Ok(p) => {
            println!("size: {} ({:?} bytes)", fmt_bytes(p.length), p.length);
            println!("ranges: {}\netag: {:?}\nlast-modified: {:?}", p.accepts_ranges, p.validators.etag, p.validators.last_modified);
            println!("file: {:?}\ntype: {:?}", p.file_name, p.mime);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

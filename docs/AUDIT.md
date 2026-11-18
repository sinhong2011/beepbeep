# BeepBeep: plan audit and decisions

Source: `nextgen-download-platform-plan-agpl-v3-video-torrent.html` (v3.0, 44 sections).
Date: 2026-11-18.

## 1. What the plan gets right

- **Reliability over speed claims** (§1, §18, §30). It bans "X times faster" marketing and requires a test and benchmark behind every engine improvement.
- **Separates the engine from the UI** (§16, §21, §23). It uses a replaceable engine interface, typed commands and events, and coalesced progress updates.
- **Explicit task lifecycle** (§20). It names queued, probing, downloading, verifying, completed, paused, retrying, failed, cancelled and seeding as distinct states, instead of one boolean "isDownloading".
- **Media as a task family** (§14, §35–38). Media is resolve → select → fetch → remux, with yt-dlp and FFmpeg as supervised external tools, not linked code.
- **Safety** (§43). It redacts secrets, uses cookies only with explicit consent, and allows no DRM or paywall bypass.

## 2. Problems found in the plan

| # | Issue | Severity | Resolution in this repo |
|---|-------|----------|-------------------------|
| A1 | **NSH APIs must be verified, not guessed** (§33). NSH is Native Shell (nsh.dev, Apache-2.0, v0.6.0). | Resolved | Built on NSH as §22 recommends: the core in Rust (`beep-core`, exported to TSX as the `Engine` module), every screen in TSX on `nsh/base` elements. API names were checked against the generated `.nsh/types` declarations and the NSH sources. |
| A2 | **The MVP scope contradicts the rest of the plan.** §32 defers video extraction, but §35/§44 make it M2, and the product request is X/Threads/Instagram video. | High | Video-by-link is in scope now, limited to *public posts by URL*. Discovery, creator browsing and account mode (§35) stay deferred. |
| A3 | **No provider fallback order is defined.** §36 defines `VideoProvider` but not what happens when a native resolver breaks, which happens often on social platforms. | High | A `ResolverChain` tries a native resolver first, then yt-dlp. Errors are classified as `loginRequired`, `notFound`, `noVideo`, `rateLimited` or `toolMissing`, each with a user-facing fix. |
| A4 | **Expiring media URLs.** §18 lists "expiring URL refresh" only as research, yet signed CDN URLs (Instagram, Threads) expire in hours, so resume fails after a pause. | High | Media tasks keep their *page* URL. On HTTP 403/410 during resume, the task re-resolves once and continues from its saved segments if the size still matches. |
| A5 | **Undefined behaviour when a server ignores Range on resume.** §18 mentions it, but no rule is given. | Medium | Every ranged request sends `If-Range` (ETag, or Last-Modified). A `200` reply on a non-zero offset restarts the file from zero; data is never silently corrupted. |
| A6 | **SQLite is required in M1** (§24). For an MVP with fewer than 10k tasks that is premature. | Low | Use an atomic JSON task store (write to temp, then rename) behind a `TaskStore` type, so SQLite can replace it without UI changes. |
| A7 | **No UI-cadence number.** §23 says "bounded cadence" with no value. | Low | The engine accumulates bytes; the manager samples at 4 Hz and smooths speed with an EMA. ETA comes from the smoothed speed. |
| A8 | **No concurrency defaults.** | Low | 3 concurrent tasks, 8 connections per task, a 1 MiB minimum segment, and a per-host cap of 16 connections. |
| A9 | **Torrent and streaming sections (§39–42) are large and unbounded.** | Info | Not implemented. They need a librqbit spike first; see the roadmap below. |

## 3. Architecture as built

```
Cargo.toml                 # workspace: engine + CLI
crates/
  beep-core/               # UI-free service (§21)
    src/domain.rs          # DownloadTask, TaskStatus, Segment, MediaInfo (§20)
    src/engine.rs          # probe, planner, segmented downloader, rate limiter (§17-18)
    src/providers/         # X, Threads, Instagram, yt-dlp, ResolverChain (§36-37)
    src/manager.rs         # queue, scheduler, 4 Hz sampling, atomic JSON store (§23-24)
    tests/core.rs
  beep-cli/                # `beep` CLI with --json, and `beep mcp` for AI assistants (§27)
app/                       # NSH desktop app (§22)
  src/lib.rs, src/glyph.rs # Rust: Engine module + Glyph component, exported as beepbeep/native
  App.tsx, store.ts        # TSX entry and UI state
  pages/, components/      # TSX screens and the app's styled nsh/base controls
  theme.json               # every color, light and dark
scripts/                   # fault-injection server + suite, icon cutter
docs/                      # this audit, DESIGN.md
AGENTS.md, llms.txt        # for coding agents and AI assistants
```

### Engine (HTTP/HTTPS)
1. **Probe.** Sends HEAD, falling back to `GET Range: bytes=0-0` because many CDNs reject HEAD. It records size, `Accept-Ranges`, ETag, Last-Modified and the filename (Content-Disposition, then URL path).
2. **Plan.** `SegmentPlanner` splits the file into N segments of at least `minSegmentSize`. Small or non-ranged files use one stream.
3. **Fetch.** One `URLSession` runs per task, with a serial delegate queue and positional writes into `<name>.beeppart`.
4. **Dynamic split (work stealing).** When a connection goes idle, the largest remaining segment is split in half and the free connection takes the tail. The old request is clamped at the new boundary.
5. **Retry.** Each segment retries with exponential backoff (0.5 s × 2ⁿ, max 6 tries) for timeouts, connection loss, 5xx and 429. A 4xx fails the task and is classified.
6. **Rate limit.** A global token bucket throttles the delegate stream by suspending tasks, so it applies to all tasks together.
7. **Resume.** Segment progress is persisted with the task. On resume the probe re-checks validators, and a size, ETag or Last-Modified change restarts the download (A5).
8. **Finish.** The download checks disk space before starting, verifies the final size, optionally computes SHA-256, moves the file atomically with collision-safe naming (`name (1).mp4`), and sends a notification.

### Media by link

| Platform | URL patterns | Native resolver | Fallback |
|----------|--------------|-----------------|----------|
| X / Twitter | `x.com`, `twitter.com`, `mobile.twitter.com`, `fxtwitter`, `vxtwitter`, `fixupx` `/…/status/<id>` | Public embed (syndication) JSON: picks the highest-bitrate MP4, and handles multi-video posts and GIFs | yt-dlp |
| Threads | `threads.net`, `threads.com` `/@user/post/<code>` | Public post HTML: `video_versions` / `og:video` | yt-dlp |
| Instagram | `instagram.com/(p\|reel\|reels\|tv)/<code>` | Public post HTML: `og:video` / `video_url` / `video_versions` | yt-dlp (opt-in `--cookies-from-browser`) |
| Anything else yt-dlp supports | any | n/a | yt-dlp |

Measured with `scripts/fault-test.sh` (all pass, SHA-256 identical): 64 MB with 8 connections;
20% dropped connections plus 10% HTTP 503; 5% stalled half-open connections (10 s stall
detection); a server without Range; interrupt and resume; and a file that changed between runs.

Direct MP4 results go through the segmented engine, so they get resume and multiple connections. Formats that need an audio/video merge are handed to a supervised `yt-dlp` process, with progress parsing and cancellation, and FFmpeg when one is found.

**Boundaries (§43):** only public posts or content the user is logged in to view; no DRM, paywall or private-account bypass; cookies only when the user turns them on; signed URLs and cookies are redacted from the task log.

## 4. Roadmap after this cut
- M3: fault-injection CI (the `scripts/rangeserver.py` scenarios as XCTest), per-host adaptive caps, SQLite store.
- M4: browser extension + native-messaging host; Link Collector rules (rename/destination templates).
- M5: librqbit torrent adapter spike, then the playback coordinator (§39–40).
- M6: done locally: one queue per user, owned by the first BeepBeep process and served on a private
  Unix socket (mode 0600) that the app, the CLI and `beep mcp` share, with takeover when the owner
  quits. Next: remote access with pairing and scoped tokens over TLS (§27), and a Windows named pipe.

## 5. Licensing
The project is AGPL-3.0-or-later (§28). No MDM, Motrix or VideoDown code is copied: the engine is written independently from RFC 9110 range semantics. yt-dlp (Unlicense) and FFmpeg (LGPL/GPL) are invoked as separate programs and are not bundled.

# BeepBeep

A desktop download manager built on [NSH](https://nsh.dev): fast, resumable downloads, and videos from
post links on **X, Threads and Instagram**. AI assistants can drive it over MCP.

- **Segmented downloads**: up to 32 connections per file, work stealing when a connection frees up,
  retries with backoff, stalled-connection detection, `If-Range` validation, atomic finish, optional
  SHA-256. Verified byte-for-byte under injected faults (`scripts/fault-test.sh`).
- **Video by link**: paste a post link; BeepBeep finds the video (X's public embed data, Threads and
  Instagram public pages, yt-dlp as fallback) and downloads the quality you choose.
- **Workspace UI**: views and sources, a paste-and-go link bar, a dense table with live progress, an
  inspector with the engine's segments and log, light and dark themes, native motion.
- **AI-friendly**: `beep mcp` (Model Context Protocol), `--json` on every CLI command, `AGENTS.md`, `llms.txt`.

## Run

```sh
brew install longbridge/tap/nsh        # the NSH toolchain
cd app && nsh run --target macos       # desktop app (TSX hot reload)
cargo run -p beep-cli -- get <url>     # CLI
cargo run -p beep-cli -- mcp           # MCP server on stdio
```

Optional: `brew install yt-dlp ffmpeg` for posts that need a login (with your browser's cookies, if
you turn that on) and for formats that need merging.

## Develop

See [AGENTS.md](AGENTS.md) for the layout, commands and rules, [docs/AUDIT.md](docs/AUDIT.md) for the
plan audit and decisions, and [docs/DESIGN.md](docs/DESIGN.md) for the UI and its motion inventory.

Download only what you have the right to. BeepBeep does not bypass DRM, paywalls or private accounts.

License: AGPL-3.0-or-later.

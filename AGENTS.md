# BeepBeep — guide for coding agents

BeepBeep is a desktop download manager: segmented HTTP(S) downloads with resume, plus video from
post links on X, Threads and Instagram. AGPL-3.0-or-later. Read `docs/AUDIT.md` for the plan audit
and design decisions before larger changes.

## Layout

| Path | What | Language |
|---|---|---|
| `crates/beep-core/` | The download service: engine, media resolvers, queue, store. No UI. | Rust |
| `crates/beep-cli/` | `beep` CLI and `beep mcp` (MCP server for AI assistants). | Rust |
| `app/` | The NSH desktop app. `src/lib.rs` exports `Engine` and `Glyph` to TSX as `beepbeep/native`. | Rust + TSX |
| `app/App.tsx`, `app/pages/`, `app/components/ui.tsx`, `app/store.ts` | Screens, the app's styled controls, UI state. | TSX |
| `app/theme.json` | Every color the UI uses (shadcn `cssVars` + app tokens). | JSON |
| `scripts/rangeserver.py`, `scripts/fault-test.sh` | Fault-injection server and end-to-end engine checks. | Python, sh |
| `scripts/cut-icon.swift` | Cuts the app icon out of the source artwork. | Swift |

## Commands

```sh
cargo test -p beep-core                 # unit tests (planner, resolvers, parsing, safety)
cargo build -p beep-cli && scripts/fault-test.sh   # engine under drops, 503s, stalls, no-Range, resume, file change
cd app && nsh check                     # type-check TSX against generated declarations (.nsh/types)
cd app && nsh run --target macos        # dev host; TSX edits hot-reload, Rust edits rebuild
cd app && nsh package App.tsx --target macos   # .nsh/out/BeepBeep.app
```

## Rules

- **Engine changes need a test and a fault-test run.** The engine must keep the file byte-identical
  (SHA-256) under every scenario in `scripts/fault-test.sh`.
- **UI follows NSH's guides**: `nsh/base` elements (Tabs, Progress, Switch, Dialog, Popover,
  AlertDialog) styled in `components/ui.tsx`; colors only through theme tokens (`bg-primary`,
  `text-ink-3`, …), never raw hex in TSX; motion from the tokens in `components/ui.tsx`; `If`/`Each`
  instead of `cond ? a : b` for elements that should survive the 4 Hz progress tick.
- **Never invent NSH APIs**: check `app/.nsh/types/**/*.d.ts` and https://nsh.dev/llms.txt.
- **Rust ↔ TSX**: add functions to `Engine` in `app/src/lib.rs` with a doc comment (it becomes the
  TypeScript doc); run `nsh check` to regenerate `.nsh/types/beepbeep/native.d.ts`.
- **Safety** (plan §43): no DRM, paywall or private-account bypass; browser cookies only when the user
  picks a browser in Settings; signed URLs are redacted from logs (`util::redact`).

## Driving BeepBeep from an AI assistant

`beep mcp` speaks the Model Context Protocol over stdio. Tools: `add_download`, `list_downloads`,
`get_download`, `pause_download`, `resume_download`, `remove_download` (destructive with
`delete_file`), `resolve_link`, `get_status`. It shares the user's one queue with the desktop app:
the first BeepBeep process to start owns the queue and serves it on a private local socket
(`crate::service`), the others attach, and an attached process takes over if the owner quits. So
what an assistant adds shows up in the window live. `--state DIR` gives a private queue instead;
`BEEPBEEP_DATA_DIR` moves the whole data folder (useful for tests).

```json
{ "mcpServers": { "beepbeep": { "command": "/path/to/beep", "args": ["mcp"] } } }
```

Scripts can use `--json` on `beep get | list | resolve | probe` (progress on stderr, one JSON
document on stdout).

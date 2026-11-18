# BeepBeep desktop app

The NSH app: TSX screens on `nsh/base` elements, with the download service in Rust.

| File | What |
|---|---|
| `App.tsx` | Entry: window, shortcuts, toast. |
| `pages/Workspace.tsx` | Sidebar, link bar, download table, inspector, status bar. |
| `pages/Dialogs.tsx` | Add Downloads and Settings dialogs. |
| `components/ui.tsx` | The app's styled controls and motion tokens. |
| `store.ts` | UI state; mirrors the engine's snapshot at 4 Hz. |
| `src/lib.rs` | `Engine` module and `Glyph` component, imported by TSX as `beepbeep/native`. |
| `theme.json` | Every color, light and dark. |
| `assets/icon/` | App icon: `source.webp` and the PNGs `scripts/cut-icon.swift` makes from it. |

```sh
nsh run --target macos              # dev host; TSX edits hot-reload, Rust edits rebuild
nsh check                           # type-check TSX against .nsh/types (generated from src/lib.rs)
nsh test                            # headless UI tests (App.test.tsx)
nsh package App.tsx --target macos  # .nsh/out/BeepBeep.app
```

See `../AGENTS.md` for the whole repository and its rules.

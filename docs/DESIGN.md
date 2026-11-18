# BeepBeep desktop design

## Shape: a downloads workspace

A download manager is a list you scan, act on and leave running, so the window is built around the
list (plan §8–11), not around artwork:

```
┌ Sidebar ─────────┬ Toolbar: view title · search · Pause/Resume All · details · settings ──────┐
│ New Download     ├ Link bar: paste a link, Return ─────────────────────────────────────────────┤
│ Library (Tabs)   │ Table: name · progress · size · speed · left · action  │ Inspector          │
│  All / Downloading│ (virtualized List, one row per download)              │ preview, actions,  │
│  Completed/Failed│                                                         │ facts, segments,   │
│ Sources          │                                                         │ activity log       │
│  X / Threads /   │                                                         │                    │
│  Instagram/Files ├ Status bar: total speed · running/waiting · folder · free space ──────────────┤
│ Throughput card  │
└──────────────────┘
```

- **The fastest path is one paste**: the link bar takes a link (or text full of links), shows what it
  is ("X video", "File", "3 links"), and Return queues it. ⌘V outside a text field does the same from
  the clipboard. "New Download" opens the batch dialog with a quality choice.
- **Rows are dense and scannable**: thumbnail or kind tile, title, one status line (platform, author,
  quality, or the error in red), a progress bar coloured by state, size, speed, time left, and one
  quick action whose glyph follows the state (pause, resume, retry, show in Finder).
- **The inspector explains**: what the engine is doing (segments and open connections), where the file
  is, and the activity log. It hides with ⌘I, and the table gets the Speed/Left columns back.
- **Status is always visible**: throughput in the sidebar and status bar; counts on every view and source.

## Elements

All interactive pieces are `nsh/base` elements styled in `app/components/ui.tsx`:
`Tabs`/`Tab` (views, segmented choices; one indicator springs to the selection), `Progress`
(role, range and value for screen readers), `Switch`, `Button`, `Input`, `Dialog` (Add, Settings),
`AlertDialog` (remove confirmation), plus `List` (virtualized rows), `Image`, `Scroll`, and the app's
own `Glyph` component (Rust, SVG line icons).

## Tokens

Colors live only in `app/theme.json`: shadcn's roles (background, foreground, primary, muted, border…)
plus the app's (`success`, `warning`, `info`, `selection`, `hover`, `track`, `surface`,
`surface-raised`, `line`, `ink-3`, `pill`, `scrim`, platform colors). Light and dark are both designed;
the palette follows the plan's "Quiet Precision" (warm neutral canvas, ink text, restrained cobalt).

Type: 20 pt view title, 14 pt row titles, 12.5 pt body, 11.5 pt meta; tabular numerals everywhere a
number changes. Radii: 9–10 pt controls, 14 pt panels, 18 pt dialogs.

## Motion inventory

Tokens (`components/ui.tsx`): instant 120 ms, fast 180 ms, base 240 ms, slow 340 ms; ease-out entering,
ease-in leaving, a soft settle `cubic-bezier(0.22, 1, 0.36, 1)` for presses.

| Change | Motion | Reduced motion |
|---|---|---|
| Press on any button, tab or row | Scale 0.97 (icons 0.94), fast, soft settle | No scale |
| Hover on buttons and rows | Wash (`hover` token) fades in, instant | Same |
| View (All/Downloading/…) changes | One selection springs to the new view (Tabs indicator); the rows glide (`LayoutAnimation`) | Instant |
| Segmented choice changes | One pill springs to the new option | Instant |
| Source filter toggles | Selection wash fades; rows glide | Instant |
| Rows arrive (first load, links added) | Rise 6 pt and fade in, base, 30 ms apart, capped at the 10th row | Instant |
| Progress advances (4 Hz samples) | Indicator width eases linearly over 300 ms between samples | Same, by design (it is data) |
| Status glyph or row action changes | Icon swaps; status colour fades | Instant |
| Size unknown (resolving, connecting) | Indeterminate bar pulses | Still bar |
| Inspector opens | Slides in 24 pt from the right and fades, base | Instant |
| Selection changes | Inspector body rises 6 pt and fades in, base | Instant |
| Pause All ↔ Resume All | Fades in, fast | Instant |
| Link bar recognizes a link | Border colours to primary; kind label fades in, fast | Same |
| Dialog opens / closes | Backdrop fades; card rises 10 pt from 0.97, slow; leaves with the dialog's fade | Instant |
| Settings section changes | Pane rises 4 pt and fades in, fast | Instant |
| Toast | Slides 24 pt up from the bottom edge and fades, slow | Instant |
| Switch toggles | Thumb glides 18 pt on the soft settle; track colour fades | Instant |

Checked in the running app on macOS: press, view change, rows arriving, progress, inspector, dialogs,
toast. Not yet checked: Windows, Linux, the web build.

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Glyph`: the app's line icons for the TSX screens (TSX has no inline SVG element; an `svg()` in a
//! Rust component draws sharp at every scale). Stroked with `currentColor`: give it a `text-*` class.

use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

/// The props of [`Glyph`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct GlyphProps {
    /// The icon: queue, download, paste, plus, settings, search, pause, play, stop, folder, close,
    /// retry, link, details, trash, check, alert, clock, grid, file.
    pub name: Prop<SharedString>,
    /// Its size in points (16 when left out).
    #[prop(default = 16.0)]
    pub size: Prop<f32>,
    /// Tailwind classes of the icon, such as its color (`text-muted-foreground`).
    pub icon_class: Prop<Option<SharedString>>,
}

fn body(name: &str) -> &'static str {
    match name {
        "queue" => r#"<path d="M12 4v10"/><path d="m7.5 9.5 4.5 4.5 4.5-4.5"/><path d="M5 19h14"/>"#,
        "download" => r#"<path d="M12 4v10"/><path d="m7.5 9.5 4.5 4.5 4.5-4.5"/><path d="M5 19h14"/>"#,
        "paste" => r#"<rect x="6" y="5" width="12" height="16" rx="2.5"/><path d="M9.5 3.5h5a1 1 0 0 1 1 1V6a.5.5 0 0 1-.5.5H9a.5.5 0 0 1-.5-.5V4.5a1 1 0 0 1 1-1z"/><path d="M9.5 12h5M9.5 15.5h3.5"/>"#,
        "plus" => r#"<path d="M12 5v14M5 12h14"/>"#,
        "settings" => r#"<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>"#,
        "search" => r#"<circle cx="11" cy="11" r="6"/><path d="m20 20-4.2-4.2"/>"#,
        "pause" => r#"<path d="M9 6v12M15 6v12"/>"#,
        "play" => r#"<path d="M8 5.5v13l10.5-6.5z"/>"#,
        "stop" => r#"<rect x="7" y="7" width="10" height="10" rx="2"/>"#,
        "folder" => r#"<path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/>"#,
        "close" => r#"<path d="m6.5 6.5 11 11M17.5 6.5l-11 11"/>"#,
        "retry" => r#"<path d="M19 12a7 7 0 1 1-2.05-4.95"/><path d="M19 4.5v4h-4"/>"#,
        "link" => r#"<path d="M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1"/><path d="M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1"/>"#,
        "details" => r#"<path d="M9 7h11M9 12h11M9 17h11"/><circle cx="4.5" cy="7" r=".6"/><circle cx="4.5" cy="12" r=".6"/><circle cx="4.5" cy="17" r=".6"/>"#,
        "trash" => r#"<path d="M4.5 7h15M10 4h4M6.5 7l1 12a2 2 0 0 0 2 1.8h5a2 2 0 0 0 2-1.8l1-12"/>"#,
        "check" => r#"<circle cx="12" cy="12" r="8.5"/><path d="m8.5 12.2 2.4 2.4 4.6-4.8"/>"#,
        "alert" => r#"<path d="M12 4 3 19.5h18z"/><path d="M12 10v4.5M12 17.2v.3"/>"#,
        "clock" => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>"#,
        "grid" => r#"<rect x="4.5" y="4.5" width="6" height="6" rx="1.5"/><rect x="13.5" y="4.5" width="6" height="6" rx="1.5"/><rect x="4.5" y="13.5" width="6" height="6" rx="1.5"/><rect x="13.5" y="13.5" width="6" height="6" rx="1.5"/>"#,
        "file" => r#"<path d="M7 3.5h6.5L18 8v11a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 19V5a1.5 1.5 0 0 1 1-1.5z"/><path d="M13.5 3.5V8H18"/>"#,
        _ => r#"<circle cx="12" cy="12" r="2"/>"#,
    }
}

fn markup(name: &str) -> String {
    let fill = if name == "play" { "currentColor" } else { "none" };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="{fill}" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        body(name)
    )
}

/// A line icon by name.
#[component]
pub fn Glyph(props: GlyphProps) -> impl IntoElement {
    svg(move || ImageSrc::svg_markup(&markup(&props.name.get())))
        .size(move || px(props.size.get()))
        .app_class(props.icon_class)
}

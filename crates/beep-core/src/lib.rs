// SPDX-License-Identifier: AGPL-3.0-or-later
//! BeepBeep core: the download service shared by the NSH desktop app, the `beep` CLI and a future
//! headless service (plan §21). It has no UI dependency.

pub mod domain;
pub mod engine;
pub mod manager;
pub mod providers;
pub mod service;
pub mod settings;
pub mod util;

pub use domain::*;
pub use manager::{Event, Manager, Snapshot, Store, default_store, is_media_link};
pub use service::{Role, Service};
pub use settings::{CookieBrowser, Quality, Settings};

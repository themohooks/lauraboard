//! Operator GUI (Slint, Material 3). It is a pure API client: everything
//! goes through the HTTP/WebSocket API, so the GUI can control a local or
//! remote instance and never blocks playout.
//!
//! * [`shell`] — window setup; the interface itself lives in `ui/` (`.slint`)
//! * [`bridge`] — one controller per screen: server data → UI models,
//!   UI callbacks → API requests
//! * [`logic`] — pure, unit-tested helpers (timeline, placement, rules, …)
//! * [`net`] — background requests, polling and the event stream
//! * [`model`] — client-side cache of server data
//! * [`fmt`] — shared formatting
//! * [`prefs`] / [`sysprefs`] — persisted UI preferences, OS accessibility hints

mod bridge;
#[cfg(feature = "snapshot")]
pub mod devtools;
mod fmt;
mod logic;
mod model;
mod net;
mod notify;
mod prefs;
mod ru;
mod shell;
mod single;
mod sysprefs;
mod tray;

/// How to reach the API.
#[derive(Debug, Clone)]
pub struct GuiOptions {
    /// e.g. `http://127.0.0.1:8787`
    pub base_url: String,
    pub token: Option<String>,
    /// Preferred channel (default: first).
    pub channel: Option<String>,
    /// The engine runs inside this process: quitting stops the broadcast.
    pub embedded_engine: bool,
}

/// Open the main window (blocks until the user quits; with a tray icon
/// closing the window only hides it).
pub fn run(opts: GuiOptions) -> Result<(), String> {
    shell::run(opts)
}

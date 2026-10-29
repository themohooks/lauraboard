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
//! * [`update`] — new versions from GitHub releases
//! * [`prefs`] / [`sysprefs`] — persisted UI preferences, OS accessibility hints

mod bridge;
#[cfg(feature = "snapshot")]
pub mod devtools;
mod fmt;
mod file_dialog;
#[cfg(any(target_os = "android", test))]
mod connection;
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::run as run_android;
#[cfg(test)]
mod input_tests;
#[cfg(not(target_os = "android"))]
mod keys;
mod logic;
mod model;
mod net;
#[cfg(not(target_os = "android"))]
mod notify;
#[cfg(target_os = "android")]
#[path = "notify/android.rs"]
mod notify;
mod pointer;
mod prefs;
mod ru;
mod shell;
#[cfg(not(target_os = "android"))]
mod single;
mod sysprefs;
mod theme_transition;
mod tray;
mod update;
mod weblogin;

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
    #[cfg(not(target_os = "android"))]
    { shell::run(opts) }
    #[cfg(target_os = "android")]
    { let _ = opts; Err("Use run_android after initializing the Android platform".into()) }
}

/// Used by the supervisor after the application process has exited.
pub fn show_crash_report(log: &str, path: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    { let _ = (log, path); return Err("Desktop crash supervisor is unavailable on Android".into()); }
    #[cfg(not(target_os = "android"))]
    {
    use slint::ComponentHandle;
    // The application may have crashed in its graphics driver. Reporting
    // must be able to open without initializing GPU rendering again.
    slint::BackendSelector::new().backend_name("winit".into()).renderer_name("software".into()).select().map_err(|e| e.to_string())?;
    let window = shell::CrashWindow::new().map_err(|e| e.to_string())?;
    window.set_log(log.into());
    window.set_log_path(path.into());
    window.run().map_err(|e| e.to_string())
    }
}

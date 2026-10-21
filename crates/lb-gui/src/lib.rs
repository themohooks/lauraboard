//! Operator GUI (egui). It is a pure API client: everything goes through the
//! HTTP/WebSocket API, so the GUI can control a local or remote instance and
//! never blocks playout.
//!
//! * [`net`] — background requests, polling and the event stream
//! * [`model`] — client-side cache of server data
//! * [`app`] — window layout, navigation, dialogs
//! * [`views`] — one module per screen
//! * [`widgets`] — shared formatting and small widgets
//! * [`prefs`] / [`sysprefs`] — persisted UI preferences, OS accessibility hints
//! * [`shell`] — Material 3 interface (Slint, `ui/`); replaces the egui views
//!   step by step. Until it is complete the egui GUI stays the default and the
//!   new one is enabled with `LB_GUI=m3`.

mod app;
mod model;
mod net;
mod prefs;
mod shell;
mod sysprefs;
mod views;
mod widgets;

/// How to reach the API.
#[derive(Debug, Clone)]
pub struct GuiOptions {
    /// e.g. `http://127.0.0.1:8787`
    pub base_url: String,
    pub token: Option<String>,
    /// Preferred channel (default: first).
    pub channel: Option<String>,
}

/// Open the main window (blocks until it is closed).
pub fn run(opts: GuiOptions) -> Result<(), String> {
    if std::env::var("LB_GUI").is_ok_and(|v| v == "m3") {
        return shell::run(opts);
    }
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Laurabrod — TV playout")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("Laurabrod", native, Box::new(move |cc| Ok(Box::new(app::App::new(cc, opts))))).map_err(|e| e.to_string())
}

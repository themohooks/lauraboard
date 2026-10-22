//! Material 3 interface built with Slint (`ui/app.slint`).

use std::rc::Rc;

use slint::{CloseRequestResponse, ComponentHandle};

use crate::GuiOptions;
use crate::bridge;
use crate::single;
use crate::prefs::UiPrefs;
use crate::tray::{self, TrayAction};

slint::include_modules!();

/// Open the main window; returns when the user quits (tray menu, or the
/// close button when there is no tray or hiding is switched off).
pub fn run(opts: GuiOptions) -> Result<(), String> {
    // already open (maybe hidden in the tray): bring that window back
    if single::activate_running(&opts.base_url) {
        return Ok(());
    }
    let prefs = UiPrefs::load();
    let window = AppWindow::new().map_err(|e| e.to_string())?;
    bridge::start(&window, &opts, prefs);
    let port = single::listen(opts.base_url.clone(), show_window);

    let icon = tray::start(Rc::new(|a| match a {
        TrayAction::Show => show_window(),
        TrayAction::Quit => {
            let _ = slint::quit_event_loop();
        }
    }));
    bridge::set_tray(icon.is_some());
    window.window().on_close_requested(|| {
        if !bridge::close_to_tray() {
            let _ = slint::quit_event_loop();
        }
        CloseRequestResponse::HideWindow
    });

    window.show().map_err(|e| e.to_string())?;
    // hidden windows do not end the loop: only an explicit quit does
    let result = slint::run_event_loop_until_quit().map_err(|e| e.to_string());
    let _ = window.hide();
    drop(icon);
    single::release(port);
    bridge::stop();
    result
}

/// Show the window (hidden in the tray or minimized) and raise it.
fn show_window() {
    bridge::with(|c| {
        if let Some(w) = c.window() {
            let _ = w.show();
            w.window().set_minimized(false);
        }
    });
}

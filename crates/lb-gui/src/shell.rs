//! Material 3 interface built with Slint (`ui/app.slint`).

use std::rc::Rc;

use slint::{CloseRequestResponse, ComponentHandle};

use crate::GuiOptions;
use crate::bridge;
use crate::prefs::UiPrefs;
#[cfg(not(target_os = "android"))]
use crate::single;
use crate::tray::{self, TrayAction};

slint::include_modules!();

/// Open the main window; returns when the user quits (tray menu, or the
/// close button when there is no tray or hiding is switched off).
#[cfg(not(target_os = "android"))]
pub fn run(opts: GuiOptions) -> Result<(), String> {
    // already open (maybe hidden in the tray): bring that window back
    if single::activate_running(&opts.base_url) {
        return Ok(());
    }
    crate::notify::select_backend();
    crate::notify::set_on_open(show_window);
    let prefs = UiPrefs::load();
    let window = AppWindow::new().map_err(|e| e.to_string())?;
    crate::keys::install(window.window());
    bridge::start(&window, &opts, prefs);
    let port = single::listen(opts.base_url.clone(), show_window);

    let embedded = opts.embedded_engine;
    let has_tray = tray::start(Rc::new(move |a| on_tray(a, embedded)));
    bridge::set_tray(has_tray);
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
    tray::stop();
    single::release(port);
    bridge::stop();
    result
}

/// A tray menu entry: actions that ask for confirmation or need input
/// bring the window up first.
fn on_tray(a: TrayAction, embedded_engine: bool) {
    let state = tray::state();
    match a {
        TrayAction::Show => show_window(),
        TrayAction::Section(id) => {
            show_window();
            bridge::with(move |c| bridge::shell::navigate(c, id));
        }
        TrayAction::Transport(t) => {
            // Windows menus resolve the state-dependent entries here
            let t = match t {
                "start-stop" if state.on_air => "stop",
                "start-stop" => "start",
                "manual-toggle" if state.manual => "manual-off",
                "manual-toggle" => "manual-on",
                t => t,
            };
            if t == "stop" {
                show_window();
            }
            bridge::with(move |c| bridge::air::transport(c, t));
        }
        TrayAction::Dialog(n) => {
            show_window();
            bridge::with(move |c| {
                if let Some(ui) = c.window() {
                    ui.global::<Air>().set_action_dialog(n);
                }
            });
        }
        TrayAction::ToggleCloseToTray => bridge::toggle_close_to_tray(),
        TrayAction::Quit if embedded_engine && state.on_air => {
            show_window();
            bridge::with(|c| {
                c.ask(
                    "Выйти и остановить эфир?",
                    "Вещание идёт из этой программы: после выхода трансляция прервётся. Чтобы эфир продолжался, закройте окно — программа останется в трее.",
                    "Выйти",
                    |_| {
                        let _ = slint::quit_event_loop();
                    },
                )
            });
        }
        TrayAction::Quit => {
            let _ = slint::quit_event_loop();
        }
    }
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

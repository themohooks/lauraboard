//! Settings screen: interface preferences (saved immediately), layout
//! reset, connection and version info.

use std::time::Duration;

use slint::{ComponentHandle, SharedString};

use super::{Ctx, with};
use crate::net::Msg;
use crate::prefs::{MotionMode, ThemeMode, UiPrefs};
use crate::shell::{Air, App, AppWindow, Lib, Pl, Settings};

/// Scale is applied once the slider rests (rescaling under the pointer
/// while dragging is disorienting).
const SCALE_DELAY: Duration = Duration::from_millis(400);

#[derive(Default)]
pub struct SettingsState {
    scale_seq: u64,
    server_version: Option<String>,
    theme_timer: slint::Timer,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Settings>();
    g.on_theme_changed(|i| {
        with(move |c| {
            c.prefs.theme = match i {
                1 => ThemeMode::Light,
                2 => ThemeMode::Dark,
                _ => ThemeMode::System,
            };
            saved(c);
        })
    });
    g.on_scale_changed(|p| {
        with(move |c| {
            c.prefs.scale = (p as f32 / 100.0).clamp(0.8, 2.0);
            c.prefs.save();
            c.settings.scale_seq += 1;
            let seq = c.settings.scale_seq;
            slint::Timer::single_shot(SCALE_DELAY, move || {
                with(move |c| {
                    if c.settings.scale_seq == seq {
                        super::shell::apply_scale(c);
                    }
                })
            });
        })
    });
    g.on_compact_changed(|v| {
        with(move |c| {
            c.prefs.compact = v;
            saved(c);
        })
    });
    g.on_motion_changed(|i| {
        with(move |c| {
            c.prefs.motion = match i {
                1 => MotionMode::On,
                2 => MotionMode::Off,
                _ => MotionMode::System,
            };
            saved(c);
        })
    });
    g.on_nav_changed(|v| {
        with(move |c| {
            c.prefs.nav_expanded = v;
            if let Some(ui) = c.window() {
                ui.global::<App>().set_nav_expanded(v);
            }
            c.prefs.save();
        })
    });
    g.on_close_to_tray_changed(|v| {
        with(move |c| {
            c.prefs.close_to_tray = v;
            c.prefs.save();
            super::tray_refresh(c);
        })
    });
    g.on_notify_changed(|i, v| {
        with(move |c| {
            let n = &mut c.prefs.notify;
            match i {
                0 => n.enabled = v,
                1 => n.errors = v,
                2 => n.skipped = v,
                3 => n.outputs = v,
                4 => n.disk = v,
                5 => n.air = v,
                6 => n.programs = v,
                7 => n.only_hidden = v,
                _ => return,
            }
            c.prefs.save();
            show(c);
        })
    });
    g.on_notify_test(|| with(super::notifications::test));
    g.on_reset_layout(|| {
        with(|c| {
            reset_layout(c);
            c.prefs.save();
            c.toast("Компоновка сброшена", false);
        })
    });
    g.on_reset_all(|| {
        with(|c| {
            c.ask(
                "Сбросить все настройки?",
                "Тема, масштаб, плотность, анимации и компоновка вернутся к исходным.",
                "Сбросить",
                |c| {
                    let section = c.prefs.section.clone();
                    let persist = c.prefs.persist;
                    c.prefs = UiPrefs { section, persist, ..UiPrefs::default() };
                    reset_layout(c);
                    saved(c);
                    super::shell::apply_scale(c);
                    c.toast("Настройки сброшены", false);
                },
            )
        })
    });
}

/// Panel sizes, navigation and library view back to their defaults.
fn reset_layout(c: &mut Ctx) {
    c.prefs.panels.clear();
    c.prefs.nav_expanded = true;
    c.prefs.library_grid = false;
    let Some(ui) = c.window() else { return };
    ui.global::<Air>().set_side_width(420.0);
    ui.global::<Pl>().set_list_width(280.0);
    ui.global::<Lib>().set_grid(false);
    ui.global::<App>().set_nav_expanded(true);
    show(c);
}

/// Save and push theme / density / motion into the UI.
fn saved(c: &mut Ctx) {
    c.prefs.save();
    let Some(ui) = c.window() else { return };
    let t = ui.global::<crate::shell::Theme>();
    let was_dark = t.get_dark();
    let mode = match c.prefs.theme {
        ThemeMode::System => crate::shell::ThemeMode::System,
        ThemeMode::Light => crate::shell::ThemeMode::Light,
        ThemeMode::Dark => crate::shell::ThemeMode::Dark,
    };
    let old_mode = t.get_mode();
    let old = if old_mode != mode && c.prefs.animations() && !ui.get_theme_transition() { ui.window().take_snapshot().ok() } else { None };
    t.set_mode(mode);
    t.set_animations(c.prefs.animations());
    t.set_compact(c.prefs.compact);
    show(c);
    if was_dark != t.get_dark()
        && let Some(old) = old
        && let Ok(new) = ui.window().take_snapshot()
    {
        crate::theme_transition::start(&ui, old, new, crate::pointer::last_click());
        c.settings.theme_timer.start(slint::TimerMode::SingleShot, crate::theme_transition::DURATION, || {
            with(|c| {
                if let Some(ui) = c.window() {
                    crate::theme_transition::finish(&ui);
                }
            })
        });
    }
}

pub fn opened(c: &mut Ctx) {
    show(c);
    if c.settings.server_version.is_none() {
        c.worker.spawn(|api| {
            let v: Result<serde_json::Value, String> = api.get("/version");
            Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
                c.settings.server_version = Some(match v {
                    Ok(v) => format!(
                        "{} {} (API v{})",
                        v["name"].as_str().unwrap_or("lauraboard"),
                        v["version"].as_str().unwrap_or("?"),
                        v["api"]
                    ),
                    Err(e) => format!("недоступна: {e}"),
                });
                show(c);
            })))
        });
    }
}

/// Preferences and connection facts into the screen.
pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Settings>();
    g.set_theme(match c.prefs.theme {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    });
    g.set_scale((c.prefs.scale * 100.0).round() as i32);
    g.set_compact(c.prefs.compact);
    g.set_motion(match c.prefs.motion {
        MotionMode::System => 0,
        MotionMode::On => 1,
        MotionMode::Off => 2,
    });
    g.set_system_reduce_motion(crate::sysprefs::reduce_motion());
    g.set_nav_expanded(c.prefs.nav_expanded);
    g.set_close_to_tray(c.prefs.close_to_tray);
    let n = &c.prefs.notify;
    g.set_notify(super::vec_model(vec![n.enabled, n.errors, n.skipped, n.outputs, n.disk, n.air, n.programs, n.only_hidden]));
    g.set_has_tray(c.has_tray);
    g.set_api_url(ui.global::<App>().get_api_url());
    g.set_online(c.model.online);
    g.set_channels(SharedString::from(if c.model.channels.is_empty() { "—".to_string() } else { c.model.channels.join(", ") }));
    g.set_server_version(c.settings.server_version.clone().unwrap_or_else(|| "…".into()).into());
    g.set_app_version(env!("CARGO_PKG_VERSION").into());
    g.set_prefs_path(UiPrefs::path().map(|p| p.display().to_string()).unwrap_or_else(|| "—".into()).into());
}

//! Window-level wiring: navigation, channel, live status, preview frame,
//! overlays, preferences (theme, scale, density, motion), clock.

use std::cell::RefCell;
use std::time::Duration;

use slint::platform::WindowEvent;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

use super::{Ctx, with};
use crate::fmt::hms;
use crate::net::Frame;
use crate::prefs::ThemeMode as PrefTheme;
use crate::shell::{App, AppWindow, StatusInfo, Theme, ThemeMode};

/// Section ids in navigation order (Ctrl+1…0 follow it as well).
pub const SECTIONS: [&str; 12] =
    ["home", "air", "playlists", "library", "schedule", "jingles", "ads", "graphics", "sources", "stats", "log", "settings"];

thread_local! {
    static CLOCK: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

/// Register the shell callbacks.
pub fn connect(ui: &AppWindow) {
    let g = ui.global::<App>();
    g.on_navigate(|id| with(move |c| navigate(c, id.as_str())));
    g.on_nav_expanded_changed(|v| {
        with(move |c| {
            c.prefs.nav_expanded = v;
            c.prefs.save();
        })
    });
    g.on_select_channel(|i| {
        with(move |c| {
            let Some(ch) = c.model.channels.get(i as usize).cloned() else { return };
            c.model.channel = Some(ch.clone());
            c.poller.set_channel(&ch);
            c.thumbs.clear();
            channels_changed(c);
            c.load_all();
        })
    });
    g.on_refresh(|| {
        with(|c| {
            c.load_channels();
            c.toast("Данные обновляются…", false);
        })
    });
    g.on_confirm_result(|ok| with(move |c| c.confirm_result(ok)));
    g.on_toast_action_clicked(|| with(|c| c.toast_action_clicked()));
    g.on_toggle_fullscreen(|| {
        with(|c| {
            let Some(ui) = c.window() else { return };
            let on = !ui.global::<App>().get_monitor_fullscreen();
            ui.global::<App>().set_monitor_fullscreen(on);
            ui.window().set_fullscreen(on);
        })
    });
    g.on_panel_resized(|id, size| {
        with(move |c| {
            c.prefs.panels.insert(id.to_string(), size);
            c.prefs.save();
        })
    });
    g.on_shortcut(|name| {
        with(move |c| match c.section().as_str() {
            "playlists" => super::playlists::shortcut(c, name.as_str()),
            "library" => super::library::shortcut(c, name.as_str()),
            "schedule" => super::schedule::shortcut(c, name.as_str()),
            "jingles" | "ads" => super::rules::shortcut(c, name.as_str()),
            _ => {}
        })
    });

    // Clock + DPI watch: once a second.
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
        with(|c| {
            tick_clock(c);
            watch_scale(c);
        })
    });
    CLOCK.with(|t| *t.borrow_mut() = Some(timer));
}

pub fn navigate(c: &mut Ctx, id: &str) {
    let Some(ui) = c.window() else { return };
    let id = if SECTIONS.contains(&id) || id == "gallery" { id } else { "home" };
    ui.global::<App>().set_section(id.into());
    c.prefs.section = id.to_string();
    c.prefs.save();
    // documents may have changed elsewhere (CLI, other operator)
    if matches!(id, "playlists" | "library" | "schedule" | "jingles" | "ads" | "graphics") {
        c.load_all();
    }
    if id == "schedule" {
        super::schedule::opened(c);
    }
    if id == "jingles" || id == "ads" {
        super::rules::opened(c, id);
    }
}

/// Push preferences into the UI (startup and after settings change).
pub fn apply_prefs(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let t = ui.global::<Theme>();
    t.set_mode(match c.prefs.theme {
        PrefTheme::System => ThemeMode::System,
        PrefTheme::Light => ThemeMode::Light,
        PrefTheme::Dark => ThemeMode::Dark,
    });
    t.set_animations(c.prefs.animations());
    t.set_compact(c.prefs.compact);
    let g = ui.global::<App>();
    g.set_nav_expanded(c.prefs.nav_expanded);
    let section = c.prefs.section.clone();
    navigate(c, &section);
    apply_scale(c);
}

/// UI scale = OS scale × user scale.
pub fn apply_scale(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let target = c.system_scale * c.prefs.scale;
    if (ui.window().scale_factor() - target).abs() > 0.001 {
        ui.window().dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: target });
    }
}

/// The OS replaces our factor when DPI changes: take it as the new base.
fn watch_scale(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let current = ui.window().scale_factor();
    let expected = c.system_scale * c.prefs.scale;
    if (current - expected).abs() > 0.001 {
        c.system_scale = current;
        apply_scale(c);
    }
}

fn tick_clock(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<App>();
    let mut s = g.get_status();
    let now = c
        .model
        .status
        .now
        .filter(|_| c.model.online)
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| chrono::Local::now().format("%H:%M:%S").to_string());
    if s.clock != now.as_str() {
        s.clock = now.into();
        g.set_status(s);
    }
}

pub fn channels_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<App>();
    let list: Vec<slint::SharedString> = c.model.channels.iter().map(|s| s.into()).collect();
    g.set_channels(slint::ModelRc::new(slint::VecModel::from(list)));
    let idx = c.model.channel.as_ref().and_then(|ch| c.model.channels.iter().position(|x| x == ch)).map(|i| i as i32).unwrap_or(-1);
    g.set_channel_index(idx);
    status_changed(c);
}

/// Map the API status to the display struct.
pub fn status_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<App>();
    let s = &c.model.status;
    let state = if c.model.online { s.state.as_str() } else { "offline" };
    let state_text = match state {
        "offline" => "НЕТ СВЯЗИ",
        "on_air" => "В ЭФИРЕ",
        "starting" => "ЗАПУСК",
        "reconnecting" => "ПЕРЕПОДКЛЮЧЕНИЕ",
        "error" => "ОШИБКА",
        _ => "ОСТАНОВЛЕН",
    };
    let o = &s.output;
    let codec = if o.video_codec.is_empty() { "—" } else { o.video_codec.as_str() };
    let encoder_text = if o.running { format!("{codec} · {:.1} fps", o.stats.fps) } else { format!("Энкодер {codec}: стоп") };
    let rate = if o.stats.bitrate_kbps > 0.0 { format!("{:.0} кбит/с · ", o.stats.bitrate_kbps) } else { String::new() };
    let encoder_detail =
        format!("{rate}{:.2}x · потеряно кадров: {} · перезапусков: {}", o.stats.speed, o.stats.drop_frames, o.restarts);
    let channel_name = match c.model.channel.as_deref() {
        Some(ch) if !s.name.is_empty() => format!("{ch} — {}", s.name),
        Some(ch) => ch.to_string(),
        None => "канал не выбран".into(),
    };
    let info = StatusInfo {
        state: state.into(),
        state_text: state_text.into(),
        manual: s.manual,
        encoder_ok: o.running && o.last_error.is_none(),
        encoder_text: encoder_text.into(),
        encoder_detail: encoder_detail.into(),
        plan_warnings: s.plan_warnings as i32,
        last_error: s.last_error.clone().or_else(|| o.last_error.clone()).unwrap_or_default().into(),
        clock: g.get_status().clock,
        to_next_program: s.time_to_next_program_ms.map(|t| hms(t.max(0))).unwrap_or_default().into(),
        outputs: o.targets.join(", ").into(),
        manual_queue: s.manual_queue as i32,
        force_schedule: s.force_schedule.clone().unwrap_or_default().into(),
    };
    g.set_status(info);
    g.set_online(c.model.online);
    g.set_channel_name(channel_name.into());
}

pub fn preview_changed(c: &mut Ctx, f: Frame) {
    let Some(ui) = c.window() else { return };
    if f.rgba.len() != (f.width * f.height * 4) as usize {
        return;
    }
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height);
    let g = ui.global::<App>();
    g.set_preview(Image::from_rgba8(buf));
    g.set_has_preview(true);
}

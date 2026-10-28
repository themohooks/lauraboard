//! Preview cadence: monitor-aware client polling and the channel's frame source.
use crate::bridge::{Ctx, vec_model};
use crate::shell::Settings;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, SharedString};

const PRESETS: &[f64] = &[10.0, 15.0, 24.0, 25.0, 30.0, 50.0, 60.0, 75.0, 90.0, 100.0, 120.0, 144.0, 165.0, 180.0, 240.0, 360.0];

fn monitor_hz(c: &Ctx) -> f64 {
    c.window()
        .and_then(|ui| ui.window().with_winit_window(|w| w.current_monitor().and_then(|m| m.refresh_rate_millihertz())))
        .flatten()
        .filter(|mhz| *mhz > 0)
        .map_or(60.0, |mhz| f64::from(mhz) / 1000.0)
}

pub fn monitor_changed(c: &mut Ctx) {
    if (monitor_hz(c) - c.settings.preview_monitor_hz).abs() > 0.001 {
        show(c);
    }
}

fn number(fps: f64) -> String {
    format!("{fps:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn apply_rate(c: &Ctx) {
    let mut fps = c.prefs.effective_preview_fps(monitor_hz(c));
    if let Some(cfg) = super::super::sources::config(c) {
        fps = fps.min(cfg.preview.effective_fps(cfg.video.fps));
    }
    c.poller.set_fps(fps);
}

pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Settings>();
    let hz = monitor_hz(c);
    c.settings.preview_monitor_hz = hz;
    let mut labels: Vec<SharedString> = [1.0, 2.0, 4.0]
        .iter()
        .map(|div| {
            let label = match *div as u8 {
                1 => "Как монитор",
                2 => "Половина частоты монитора",
                _ => "Четверть частоты монитора",
            };
            format!("{label} ({} FPS)", number((hz / div).clamp(10.0, 360.0))).into()
        })
        .collect();
    labels.extend(PRESETS.iter().map(|fps| format!("{} FPS", number(*fps)).into()));
    labels.push("Своя частота".into());
    g.set_preview_options(vec_model(labels));
    if !c.settings.preview_initialized {
        let index = match c.prefs.preview_monitor_divisor {
            1 => 0,
            2 => 1,
            4 => 2,
            _ => PRESETS.iter().position(|v| (*v - c.prefs.preview_fps).abs() < 0.001).map_or(PRESETS.len() + 3, |i| i + 3),
        };
        g.set_preview_preset(index as i32);
        g.set_preview_fps(number(c.prefs.effective_preview_fps(hz)).into());
        c.settings.preview_initialized = true;
    }
    if (0..3).contains(&g.get_preview_preset()) {
        g.set_preview_fps(number((hz / [1.0, 2.0, 4.0][g.get_preview_preset() as usize]).clamp(10.0, 360.0)).into());
    }
    let source = super::super::sources::config(c).map_or("источник недоступен".into(), |cfg| {
        if cfg.preview.enabled {
            format!("источник {} FPS, видео канала {} FPS", number(cfg.preview.effective_fps(cfg.video.fps)), number(cfg.video.fps))
        } else {
            "источник предпросмотра выключен".into()
        }
    });
    g.set_preview_info(format!("Монитор {} Гц · {source}. Уникальных кадров не может быть больше, чем FPS канала.", number(hz)).into());
    apply_rate(c);
}

pub fn select(c: &mut Ctx, index: i32) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Settings>();
    g.set_preview_preset(index);
    let value = match index {
        0..=2 => Some((monitor_hz(c) / [1.0, 2.0, 4.0][index as usize]).clamp(10.0, 360.0)),
        _ => PRESETS.get(index.saturating_sub(3) as usize).copied(),
    };
    if let Some(fps) = value {
        g.set_preview_fps(number(fps).into());
    }
}

pub fn save(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Settings>();
    let Ok(fps) = g.get_preview_fps().trim().replace(',', ".").parse::<f64>() else {
        c.toast("Укажите FPS от 10 до 360", true);
        return;
    };
    if !fps.is_finite() || !(10.0..=360.0).contains(&fps) {
        c.toast("Укажите FPS от 10 до 360", true);
        return;
    }
    c.prefs.preview_fps = fps;
    c.prefs.preview_monitor_divisor = match g.get_preview_preset() {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 0,
    };
    c.prefs.save();
    c.settings.preview_initialized = false;
    if let Some(mut cfg) = super::super::sources::config(c) {
        // Monitor changes must not require an encoder restart. Produce the
        // source video cadence in adaptive mode and pace the client separately.
        let wanted = if c.prefs.preview_monitor_divisor > 0 {
            cfg.video.fps.max(1.0)
        } else {
            c.prefs.effective_preview_fps(monitor_hz(c)).min(cfg.video.fps.max(1.0))
        };
        if (cfg.preview.fps - wanted).abs() > 0.001 || !cfg.preview.enabled {
            cfg.preview.enabled = true;
            cfg.preview.fps = wanted;
            c.save_doc("/config", cfg, "Частота предпросмотра");
        } else {
            c.toast("Частота предпросмотра сохранена", false);
        }
    } else {
        c.toast("Частота интерфейса сохранена; источник канала недоступен", false);
    }
    show(c);
}

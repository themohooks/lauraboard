//! Sources & scenes overview: schedules as switchable scenes, encoder
//! output and targets (secrets masked), recording / fallback, stream media.

use lb_core::config::ChannelConfig;
use lb_core::media::MediaKind;
use serde_json::{Value, json};
use slint::{ComponentHandle, SharedString};

use super::{Ctx, vec_model, with};
use crate::logic::mask::mask_url;
use crate::shell::{App, AppWindow, Fact, OutRow, SceneRow, Src, StreamRow};

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Src>();
    g.on_switch_scene(|i| with(move |c| switch(c, i)));
    g.on_open_library(|| with(|c| super::shell::navigate(c, "library")));
}

fn fact(label: &str, value: impl Into<String>) -> Fact {
    Fact { label: label.into(), value: SharedString::from(value.into()) }
}

fn switch(c: &mut Ctx, i: i32) {
    let (id, name): (Value, String) = if i < 0 {
        (Value::Null, "Авто (по недельному плану)".into())
    } else {
        match c.model.schedules.schedules.get(i as usize) {
            Some(s) => (json!(s.id), if s.name.is_empty() { s.id.clone() } else { s.name.clone() }),
            None => return,
        }
    };
    c.ask(
        "Переключить сцену?",
        &format!("«{name}» начнёт действовать сразу: план эфира будет перестроен."),
        "Переключить",
        move |c| c.command(json!({ "cmd": "switch_schedule", "schedule": id })),
    );
}

pub fn refresh(c: &mut Ctx) {
    if c.section() != "sources" {
        return;
    }
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Src>();
    let st = &c.model.status;
    let forced = st.force_schedule.clone();

    let mut scenes = vec![SceneRow {
        index: -1,
        name: "Авто".into(),
        detail: "расписание по недельному плану и датам активации".into(),
        active: forced.is_none(),
        enabled: true,
    }];
    for (i, s) in c.model.schedules.schedules.iter().enumerate() {
        let mut detail = vec![format!("id: {}", s.id)];
        if let Some(t) = s.activate_at {
            detail.push(format!("активируется {}", t.format("%d.%m.%Y %H:%M")));
        }
        if !s.enabled {
            detail.push("выключено".into());
        }
        scenes.push(SceneRow {
            index: i as i32,
            name: if s.name.is_empty() { s.id.as_str().into() } else { s.name.as_str().into() },
            detail: detail.join(" · ").into(),
            active: forced.as_deref() == Some(s.id.as_str()),
            enabled: s.enabled,
        });
    }
    g.set_scenes(vec_model(scenes));
    g.set_manual(st.manual);

    let o = &st.output;
    g.set_running(o.running);
    g.set_state_text(ui.global::<App>().get_status().state_text);
    let mut enc = vec![fact("Кодек", if o.video_codec.is_empty() { "—".to_string() } else { o.video_codec.clone() })];
    if o.running {
        enc.push(fact("Кадров в секунду", format!("{:.1}", o.stats.fps)));
        enc.push(fact("Битрейт", format!("{:.0} кбит/с", o.stats.bitrate_kbps)));
        enc.push(fact("Скорость", format!("{:.2}×", o.stats.speed)));
        enc.push(fact("Потеряно кадров", o.stats.drop_frames.to_string()));
    }
    enc.push(fact("Перезапусков", o.restarts.to_string()));
    g.set_encoder(vec_model(enc));
    g.set_last_error(o.last_error.clone().unwrap_or_default().into());
    g.set_active_targets(vec_model(o.targets.iter().map(|t| SharedString::from(mask_url(t))).collect()));

    let cfg: Option<ChannelConfig> = c.channel_config.clone().and_then(|v| serde_json::from_value(v).ok());
    g.set_has_config(cfg.is_some());
    if let Some(cfg) = cfg {
        let v = &cfg.video;
        let a = &cfg.audio;
        let e = &cfg.encoder;
        g.set_format(vec_model(vec![
            fact("Видео", format!("{}×{}, {} к/с", v.width, v.height, v.fps)),
            fact("Звук", format!("{} Гц, {} кан.", a.sample_rate, a.channels)),
            fact(
                "Кодирование",
                format!("{} {} · {} · звук {} {}", e.video_codec, e.preset, e.video_bitrate, e.audio_codec, e.audio_bitrate).trim().to_string(),
            ),
            fact("GOP", format!("{} с", e.gop_seconds)),
        ]));
        g.set_outputs(vec_model(
            cfg.outputs
                .iter()
                .map(|t| OutRow {
                    name: t.name.as_str().into(),
                    url: mask_url(&t.url).into(),
                    format: t.format.clone().unwrap_or_default().into(),
                    enabled: t.enabled,
                })
                .collect(),
        ));
        let r = &cfg.record;
        let mut rec = vec![fact("Запись", if r.enabled { "включена" } else { "выключена" })];
        if r.enabled {
            rec.push(fact("Папка", r.dir.clone()));
            rec.push(fact("Сегменты", crate::fmt::hms(r.segment.ms())));
        }
        let fb = &cfg.fallback;
        rec.push(fact("Резервная картинка", fb.image.clone().unwrap_or_else(|| "—".into())));
        rec.push(fact("Резервный ролик", fb.media.clone().unwrap_or_else(|| "—".into())));
        g.set_recording(vec_model(rec));
    }

    g.set_streams(vec_model(
        c.model
            .library
            .iter()
            .filter(|m| m.kind == MediaKind::Stream)
            .map(|m| StreamRow {
                title: if m.title.is_empty() { m.id.as_str().into() } else { m.title.as_str().into() },
                url: mask_url(&m.path).into(),
                enabled: m.enabled,
            })
            .collect(),
    ));
}

//! Sources & scenes: schedules as switchable scenes, encoder output and
//! its targets (secrets masked; add / edit / toggle / delete with the admin
//! role), recording / fallback, live stream inputs (stream media: add,
//! edit, toggle, delete, queue or air now).

use lb_core::Dur;
use lb_core::config::{ChannelConfig, OutputTarget};
use lb_core::media::{MediaItem, MediaKind};
use serde_json::{Value, json};
use slint::{ComponentHandle, SharedString};

use super::{Ctx, vec_model, with};
use crate::logic::mask::{join_key, mask_url, split_key};
use crate::shell::{App, AppWindow, Fact, OutRow, SceneRow, Src, StreamRow};

/// Container overrides offered in the dialog (index 0 = guess from the URL).
const FORMATS: [&str; 4] = ["", "flv", "mpegts", "hls"];

/// What the edit dialog is editing (index; `None` = new).
#[derive(Default)]
pub struct SourcesState {
    editing: Option<usize>,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Src>();
    g.on_switch_scene(|i| with(move |c| switch(c, i)));
    g.on_open_library(|| with(|c| super::shell::navigate(c, "library")));
    g.on_output_edit(|i| with(move |c| output_edit(c, usize::try_from(i).ok())));
    g.on_output_toggle(|i| {
        with(move |c| {
            change_outputs(c, "Точка вещания", move |o| {
                if let Some(t) = o.get_mut(i as usize) {
                    t.enabled = !t.enabled;
                }
            })
        })
    });
    g.on_output_remove(|i| with(move |c| output_remove(c, i as usize)));
    g.on_stream_edit(|i| with(move |c| stream_edit(c, usize::try_from(i).ok())));
    g.on_stream_toggle(|i| {
        with(move |c| {
            if let Some(mut m) = stream(c, i as usize) {
                m.enabled = !m.enabled;
                put_media(c, m, "Поток сохранён");
            }
        })
    });
    g.on_stream_remove(|i| with(move |c| stream_remove(c, i as usize)));
    g.on_stream_air(|i, how| with(move |c| stream_air(c, i as usize, how.as_str())));
    g.on_dialog_save(|| with(dialog_save));
}

pub(super) fn config(c: &Ctx) -> Option<ChannelConfig> {
    c.channel_config.clone().and_then(|v| serde_json::from_value(v).ok())
}

/// Edit the output list and save the channel settings.
fn change_outputs(c: &mut Ctx, what: &'static str, f: impl FnOnce(&mut Vec<OutputTarget>)) {
    let Some(mut cfg) = config(c) else {
        c.toast("Настройки канала недоступны: нужна роль администратора", true);
        return;
    };
    f(&mut cfg.outputs);
    // show the change right away; the reload after saving confirms it
    c.channel_config = serde_json::to_value(&cfg).ok();
    refresh(c);
    c.save_doc("/config", cfg, what);
}

fn output_edit(c: &mut Ctx, i: Option<usize>) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Src>();
    let t = i.and_then(|i| config(c).and_then(|cfg| cfg.outputs.get(i).cloned()));
    let (server, key) = t.as_ref().map(|t| split_key(&t.url)).unwrap_or_default();
    g.set_e_name(t.as_ref().map(|t| t.name.clone()).unwrap_or_default().into());
    g.set_e_url(server.into());
    g.set_e_key(key.into());
    g.set_e_format(t.as_ref().and_then(|t| t.format.as_deref()).and_then(|f| FORMATS.iter().position(|x| *x == f)).unwrap_or(0) as i32);
    g.set_e_error(SharedString::new());
    g.set_dialog_new(t.is_none());
    g.set_dialog("output".into());
    c.src.editing = i.filter(|_| t.is_some());
}

fn output_remove(c: &mut Ctx, i: usize) {
    let Some(name) = config(c).and_then(|cfg| cfg.outputs.get(i).map(|t| t.name.clone())) else { return };
    let running = c.model.status.output.running;
    c.ask(
        "Удалить точку вещания?",
        &format!("«{name}» будет удалена из настроек канала.{}", if running { " Кодировщик перезапустится — короткий обрыв эфира." } else { "" }),
        "Удалить",
        move |c| {
            change_outputs(c, "Точка вещания удалена", move |o| {
                if i < o.len() {
                    o.remove(i);
                }
            })
        },
    );
}

/// Stream items in display order.
fn streams(c: &Ctx) -> Vec<&MediaItem> {
    c.model.library.iter().filter(|m| m.kind == MediaKind::Stream).collect()
}

fn stream(c: &Ctx, i: usize) -> Option<MediaItem> {
    streams(c).get(i).map(|m| (*m).clone())
}

fn put_media(c: &mut Ctx, m: MediaItem, done: &'static str) {
    let ch = c.model.ch();
    c.change(move |api| {
        api.put(&format!("/channels/{ch}/media/{}", m.id), &m)?;
        Ok(done.into())
    });
}

fn stream_edit(c: &mut Ctx, i: Option<usize>) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Src>();
    let m = i.and_then(|i| stream(c, i));
    g.set_e_name(m.as_ref().map(|m| m.title.clone()).unwrap_or_default().into());
    g.set_e_url(m.as_ref().map(|m| m.path.clone()).unwrap_or_default().into());
    g.set_e_duration(crate::fmt::hms(m.as_ref().map(|m| m.duration.ms()).filter(|d| *d > 0).unwrap_or(3_600_000)).into());
    g.set_e_error(SharedString::new());
    g.set_dialog_new(m.is_none());
    g.set_dialog("stream".into());
    c.src.editing = i.filter(|_| m.is_some());
}

fn stream_remove(c: &mut Ctx, i: usize) {
    let Some(m) = stream(c, i) else { return };
    let title = if m.title.is_empty() { m.id.clone() } else { m.title.clone() };
    c.ask("Удалить поток?", &format!("«{title}» будет удалён из медиатеки."), "Удалить", move |c| {
        let ch = c.model.ch();
        c.change(move |api| {
            api.delete(&format!("/channels/{ch}/media/{}", m.id))?;
            Ok("Поток удалён".into())
        });
    });
}

fn stream_air(c: &mut Ctx, i: usize, how: &str) {
    let Some(m) = stream(c, i) else { return };
    let title = if m.title.is_empty() { m.id.clone() } else { m.title.clone() };
    if how == "now" {
        c.ask("Показать поток сейчас?", &format!("Текущий элемент будет прерван, в эфир пойдёт «{title}»."), "В эфир", move |c| {
            c.command(json!({ "cmd": "play_now", "media": m.id }))
        });
    } else {
        c.command(json!({ "cmd": "queue", "media": m.id }));
    }
}

/// New media id from a title (latin letters, digits, '-').
fn stream_id(c: &Ctx, title: &str) -> String {
    let slug: Vec<String> = title
        .to_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect();
    let base = if slug.is_empty() { "stream".to_string() } else { format!("stream-{}", slug.join("-")) };
    let mut id = base.clone();
    let mut n = 2;
    while c.model.library.iter().any(|m| m.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn dialog_save(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Src>();
    let url = g.get_e_url().trim().to_string();
    let name = g.get_e_name().trim().to_string();
    if url.is_empty() {
        g.set_e_error("Укажите адрес".into());
        return;
    }
    let editing = c.src.editing;
    if g.get_dialog() == "output" {
        let url = join_key(&url, &g.get_e_key());
        let format = FORMATS.get(g.get_e_format().max(0) as usize).filter(|f| !f.is_empty()).map(|f| f.to_string());
        let taken = config(c).is_some_and(|cfg| cfg.outputs.iter().enumerate().any(|(k, t)| t.name == name && Some(k) != editing));
        if taken {
            g.set_e_error("Точка вещания с таким названием уже есть".into());
            return;
        }
        g.set_dialog(SharedString::new());
        change_outputs(c, "Точка вещания", move |o| {
            let n = o.len() + 1;
            let name = if name.is_empty() { format!("output-{n}") } else { name };
            match editing.and_then(|i| o.get_mut(i)) {
                Some(t) => {
                    t.name = name;
                    t.url = url;
                    t.format = format;
                }
                None => o.push(OutputTarget { name, url, enabled: true, format }),
            }
        });
    } else {
        let duration = match g.get_e_duration().trim().parse::<Dur>() {
            Ok(d) if d.ms() > 0 => d,
            _ => {
                g.set_e_error("Длительность в формате ЧЧ:ММ:СС, больше нуля".into());
                return;
            }
        };
        let title = if name.is_empty() { url.clone() } else { name };
        let m = match editing.and_then(|i| stream(c, i)) {
            Some(mut m) => {
                m.title = title;
                m.path = url;
                m.duration = duration;
                m
            }
            None => {
                let mut m = MediaItem::new(stream_id(c, &title), url, MediaKind::Stream, duration);
                m.title = title;
                m
            }
        };
        g.set_dialog(SharedString::new());
        put_media(c, m, if editing.is_some() { "Поток сохранён" } else { "Поток добавлен" });
    }
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
            fact("Кодирование", format!("{} {} · звук {} {}", e.video_codec, e.preset, e.audio_codec, e.audio_bitrate).trim().to_string()),
            fact(
                "Режим битрейта",
                match e.rate_control {
                    lb_core::config::RateControl::Cbr => format!("постоянный {}", e.video_bitrate),
                    lb_core::config::RateControl::Vbr => format!("переменный {}, до {}", e.video_bitrate, e.maxrate.as_deref().unwrap_or("1,5×")),
                    lb_core::config::RateControl::Quality => format!("качество CRF {}{}", e.crf, e.maxrate.as_deref().map(|m| format!(", до {m}")).unwrap_or_default()),
                },
            ),
            fact("Ключевые кадры", format!("каждые {} с{}", e.gop_seconds, e.bframes.map(|b| format!(" · B-кадров: {b}")).unwrap_or_default())),
        ]));
        g.set_outputs(vec_model(
            cfg.outputs
                .iter()
                .enumerate()
                .map(|(i, t)| OutRow {
                    index: i as i32,
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
            .enumerate()
            .map(|(i, m)| StreamRow {
                index: i as i32,
                title: if m.title.is_empty() { m.id.as_str().into() } else { m.title.as_str().into() },
                url: mask_url(&m.path).into(),
                enabled: m.enabled,
            })
            .collect(),
    ));
}

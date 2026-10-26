//! Home and On-Air screens: now / next, queue, quick actions, transport.

use serde_json::{Value, json};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use lb_core::graphics::ElementKind;
use lb_core::rundown::{Origin, RundownItem};

use super::{Ctx, cmd, thumbs, with};
use crate::fmt::{hms, item_kind};
use crate::shell::{Air, AppWindow, GfxRow, LogRow, MediaPick, NextInfo, NowInfo, QueueRow};

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn strings(v: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    model(v.into_iter().map(SharedString::from).collect())
}

pub fn connect(ui: &AppWindow) {
    let a = ui.global::<Air>();
    a.on_inspect_queue(|uid| with(move |c| inspect_queue(c, uid.as_str())));
    a.on_transport(|action| with(move |c| transport(c, action.as_str())));
    a.on_search_media(|_| with(media_results));
    a.on_play_now(|id| {
        with(move |c| {
            let title = c.model.library.iter().find(|m| m.id == id.as_str()).map(|m| m.title.clone()).unwrap_or_else(|| id.to_string());
            let id = id.to_string();
            c.ask(
                "Показать сейчас?",
                &format!("Текущий элемент будет прерван, в эфир пойдёт «{title}»."),
                "Показать сейчас",
                move |c| c.command(json!({ "cmd": "play_now", "media": id })),
            );
        })
    });
    a.on_queue_media(|id| with(move |c| c.command(json!({ "cmd": "queue", "media": id.as_str() }))));
    a.on_ad_break(|i, now| {
        with(move |c| {
            let Some(b) = c.model.rules.ad_breaks.get(i as usize).map(|b| b.id.clone()) else { return };
            if now {
                c.ask(
                    "Запустить рекламу сейчас?",
                    "Текущий элемент будет прерван.",
                    "Запустить",
                    move |c| c.command(json!({ "cmd": "ad_break", "ad_break": b, "now": true })),
                );
            } else {
                c.command(json!({ "cmd": "ad_break", "ad_break": b, "now": false }));
            }
        })
    });
    a.on_switch_schedule(|i| {
        with(move |c| {
            let id: Value = match i {
                0 => Value::Null,
                n => match c.model.schedules.schedules.get(n as usize - 1) {
                    Some(s) => json!(s.id),
                    None => return,
                },
            };
            c.command(json!({ "cmd": "switch_schedule", "schedule": id }));
        })
    });
    a.on_graphic(|id, action| {
        with(move |c| {
            let body = if action == "auto" {
                json!({ "cmd": "graphic_auto", "element": id.as_str() })
            } else {
                json!({ "cmd": "graphic", "element": id.as_str(), "action": action.as_str() })
            };
            c.command(body);
        })
    });
    a.on_seek_to(|f| {
        with(move |c| {
            let Some(it) = c.model.status.on_air.as_ref().and_then(|o| o.item.as_ref()) else { return };
            let to = (f.clamp(0.0, 1.0) as f64 * it.duration_ms as f64) as i64;
            c.command(json!({ "cmd": "seek", "to_ms": to }));
        })
    });
    a.on_seek_by(|s| with(move |c| c.command(json!({ "cmd": "seek", "by_ms": s as i64 * 1000 }))));
    a.on_time_of(|s| hms((s.max(0.0) * 1000.0) as i64).into());
    a.on_apply_ticker(|text| {
        with(move |c| {
            let value = if text.trim().is_empty() { Value::Null } else { json!(text.as_str()) };
            c.command(json!({ "cmd": "set_var", "key": "ticker", "value": value }));
        })
    });
}

pub fn transport(c: &mut Ctx, action: &str) {
    match action {
        "start" => c.command(cmd("start")),
        "stop" => c.ask(
            "Остановить вещание?",
            "Зрители увидят обрыв трансляции. Состояние эфира будет сохранено, продолжить можно кнопкой «Начать вещание».",
            "Остановить",
            |c| c.command(cmd("stop")),
        ),
        "skip" => c.command(cmd("skip")),
        "restart" => c.command(cmd("restart")),
        "manual-on" => c.command(json!({ "cmd": "manual", "enabled": true })),
        "manual-off" => c.command(json!({ "cmd": "manual", "enabled": false })),
        "clear-queue" => c.ask(
            "Очистить очередь оператора?",
            "Все ролики, добавленные вручную, будут убраны из очереди.",
            "Очистить",
            |c| c.command(cmd("clear_queue")),
        ),
        _ => {}
    }
}

/// Live part: now / next (every status update).
pub fn status_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let a = ui.global::<Air>();
    let s = &c.model.status;
    let now = match s.on_air.as_ref().and_then(|o| o.item.as_ref().map(|i| (o, i))) {
        Some((o, it)) => {
            let dev = o.deviation_ms / 1000;
            NowInfo {
                has_item: true,
                title: it.title.clone().into(),
                kind: item_kind(it).into(),
                block: it.block_title.clone().unwrap_or_default().into(),
                elapsed: hms(o.elapsed_ms).into(),
                remaining: hms(o.remaining_ms).into(),
                duration: hms(it.duration_ms).into(),
                progress: (o.elapsed_ms as f32 / it.duration_ms.max(1) as f32).clamp(0.0, 1.0),
                deviation: format!("{dev:+} с").into(),
                deviation_warn: dev.abs() > 10,
                started: it.start.format("%H:%M:%S").to_string().into(),
                ends: it.end().format("%H:%M:%S").to_string().into(),
                seekable: !it.media.is_empty() && !matches!(it.kind, lb_core::media::MediaKind::Stream | lb_core::media::MediaKind::Image),
                duration_s: (it.duration_ms / 1000) as i32,
            }
        }
        None => NowInfo::default(),
    };
    let next_media = s.next.as_ref().map(|n| n.media.clone()).unwrap_or_default();
    let next = NextInfo {
        has_item: s.next.is_some(),
        title: s.next.as_ref().map(|n| n.title.clone()).unwrap_or_default().into(),
        kind: s.next.as_ref().map(item_kind).unwrap_or("program").into(),
        start: s.next.as_ref().map(|n| n.start.format("%H:%M:%S").to_string()).unwrap_or_default().into(),
        duration: s.next.as_ref().map(|n| hms(n.duration_ms)).unwrap_or_default().into(),
        media: next_media.clone().into(),
        program_title: s
            .next_program
            .as_ref()
            .map(|p| match &p.block_title {
                Some(b) if !b.is_empty() => format!("{b} — {}", p.title),
                _ => p.title.clone(),
            })
            .unwrap_or_default()
            .into(),
        program_start: s.next_program.as_ref().map(|p| p.start.format("%H:%M").to_string()).unwrap_or_default().into(),
        program_in: s.time_to_next_program_ms.map(|t| hms(t.max(0))).unwrap_or_default().into(),
    };
    let current_uid = s.on_air.as_ref().and_then(|o| o.item.as_ref()).map(|i| i.uid.clone());
    let next_uid = s.next.as_ref().map(|i| i.uid.clone());
    let active: Vec<String> = s.graphics.iter().map(|g| g.element.clone()).collect();
    a.set_now(now);
    a.set_next(next);
    thumbs::request(c, &next_media);
    match c.thumbs.get(&next_media) {
        Some(img) => {
            a.set_next_thumb(img);
            a.set_has_next_thumb(true);
        }
        None => a.set_has_next_thumb(false),
    }
    if (current_uid.clone(), next_uid.clone()) != c.air_marks {
        c.air_marks = (current_uid, next_uid);
        queue_changed(c);
    }
    graphics_rows(c, &active);
}

/// Queue rows (rundown) with current / next markers.
pub fn queue_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let a = ui.global::<Air>();
    let r = &c.model.rundown;
    let (cur, nxt) = (&c.air_marks.0, &c.air_marks.1);
    let skip = cur.as_ref().and_then(|u| r.items.iter().position(|i| &i.uid == u)).unwrap_or(0);
    let mut last_block = None;
    let rows: Vec<QueueRow> = r
        .items
        .iter()
        .skip(skip)
        .take(300)
        .map(|it| {
            let block_start = it.block_key != last_block;
            last_block = it.block_key.clone();
            let why = why(&c.model, it);
            let mut row = queue_row(it, why, block_start, cur.as_deref() == Some(it.uid.as_str()), nxt.as_deref() == Some(it.uid.as_str()));
            if is_scene(&c.model, it) {
                row.kind = "scene".into();
            }
            row
        })
        .collect();
    a.set_queue(model(rows));
    a.set_queue_summary(format!("{} элементов · {}", r.items.len(), hms(r.total_ms())).into());
    a.set_warnings(strings(r.warnings.iter().map(|w| {
        let at = w.at.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default();
        let text = crate::ru::server(&w.message);
        if at.is_empty() { text } else { format!("{at} — {text}") }
    })));
}

/// Why a non-main item is in the queue: the rule (by name) that inserted
/// it, a scene covering a gap.
fn why(m: &crate::model::Model, it: &RundownItem) -> Option<String> {
    let scene = is_scene(m, it);
    let rule = it.rule.as_ref().map(|id| {
        m.rules.rules.iter().find(|r| &r.id == id).map_or(id.clone(), |r| if r.name.is_empty() { r.id.clone() } else { r.name.clone() })
    });
    match (scene, rule, it.origin) {
        (true, Some(r), _) => Some(format!("сцена по правилу «{r}»")),
        (true, None, Origin::Filler) => Some("сцена: заполнение пустоты".into()),
        (true, None, Origin::Main) => Some("сцена".into()),
        (false, Some(r), _) => Some(format!("правило «{r}»")),
        _ => None,
    }
}

fn is_scene(m: &crate::model::Model, it: &RundownItem) -> bool {
    !it.media.is_empty() && m.library.iter().any(|x| x.id == it.media && x.is_scene())
}

fn inspect_queue(c: &mut Ctx, uid: &str) {
    let Some(it) = c.model.rundown.items.iter().find(|it| it.uid == uid) else { return };
    let Some(ui) = c.window() else { return };
    let mut lines = vec![
        format!("Начало: {} · Конец: {}", it.start.format("%d.%m %H:%M:%S"), it.end().format("%H:%M:%S")),
        format!("Длительность в эфире: {}", hms(it.duration_ms)),
        format!("Блок: {}", it.block_title.as_deref().or(it.block.as_deref()).unwrap_or("—")),
        format!("Плейлист: {}", it.playlist.as_deref().unwrap_or("—")),
        format!("Категория: {}", it.category.as_deref().unwrap_or("—")),
    ];
    if let Some(why) = why(&c.model, it) {
        lines.push(format!("Причина вставки: {why}"));
    }
    if let Some(m) = c.model.library.iter().find(|m| m.id == it.media) {
        lines.push(format!("ID: {}", m.id));
        if !m.path.is_empty() {
            lines.push(format!("Файл: {}", m.path));
        }
        lines.push(format!("Полная длительность: {} · Начальная позиция: {}", hms(m.duration.ms()), hms(it.seek_ms)));
        if !m.tags.is_empty() {
            lines.push(format!("Теги: {}", m.tags.join(", ")));
        }
        let mut meta: Vec<_> = m.meta.iter().collect();
        meta.sort_by_key(|(k, _)| *k);
        for (key, value) in meta {
            lines.push(format!("{key}: {value}"));
        }
    }
    if it.truncated {
        lines.push("Ролик будет обрезан по правилу планирования".into());
    }
    let air = ui.global::<Air>();
    air.set_detail_title(it.title.clone().into());
    air.set_detail_text(lines.join("\n").into());
    air.set_detail_open(true);
}

pub fn queue_row(it: &RundownItem, why: Option<String>, block_start: bool, current: bool, next: bool) -> QueueRow {
    let mut note = Vec::new();
    if let Some(d) = it.fixed_deviation_ms() {
        note.push(if d == 0 { "фикс. время".to_string() } else { format!("фикс. {:+} с", d / 1000) });
    }
    if it.truncated {
        note.push("обрезано".into());
    }
    if let Some(w) = why {
        note.push(w);
    }
    if it.optional && it.origin != Origin::Main && it.origin != Origin::Filler {
        note.push("необязательный".into());
    }
    QueueRow {
        uid: it.uid.clone().into(),
        start: it.start.format("%H:%M:%S").to_string().into(),
        end: it.end().format("%H:%M:%S").to_string().into(),
        duration: hms(it.duration_ms).into(),
        title: it.title.clone().into(),
        kind: item_kind(it).into(),
        block: it.block_title.clone().unwrap_or_default().into(),
        note: note.join(" · ").into(),
        block_start,
        current,
        next,
        main: it.origin == Origin::Main,
    }
}

fn graphics_rows(c: &mut Ctx, active: &[String]) {
    let Some(ui) = c.window() else { return };
    let rows: Vec<GfxRow> = c
        .model
        .graphics
        .elements
        .iter()
        .map(|e| GfxRow {
            id: e.id.clone().into(),
            name: if e.name.is_empty() { e.id.clone() } else { e.name.clone() }.into(),
            kind: element_kind_name(&e.kind).into(),
            layer: e.layer.to_string().into(),
            on: active.contains(&e.id),
        })
        .collect();
    let air = ui.global::<Air>();
    // status comes several times a second: rebuilding the rows would
    // recreate the buttons between press and release and lose the click
    let current = air.get_graphics();
    if current.row_count() == rows.len() {
        for (i, r) in rows.into_iter().enumerate() {
            if current.row_data(i).as_ref() != Some(&r) {
                current.set_row_data(i, r);
            }
        }
    } else {
        air.set_graphics(model(rows));
    }
}

pub fn element_kind_name(k: &ElementKind) -> &'static str {
    match k {
        ElementKind::Image { path } if path.to_ascii_lowercase().ends_with(".swf") => "SWF анимация",
        ElementKind::Image { path } if [".html", ".htm"].iter().any(|ext| path.to_ascii_lowercase().ends_with(ext)) => "HTML анимация",
        ElementKind::Image { .. } => "изображение",
        ElementKind::Text { .. } => "текст",
        ElementKind::Ticker { .. } => "бегущая строка",
        ElementKind::Clock { .. } => "часы",
        ElementKind::Countdown { target: lb_core::graphics::CountdownTarget::ProgramRemaining, .. } => "до конца передачи",
        ElementKind::Countdown { .. } => "таймер",
        ElementKind::Progress { .. } => "прогресс передачи",
        ElementKind::Rect { .. } => "фигура",
        ElementKind::Plate { .. } => "плашка",
        ElementKind::Group { .. } => "композиция",
    }
}

/// Library search for "play now / queue".
pub fn media_results(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let a = ui.global::<Air>();
    let f = a.get_media_filter().to_lowercase();
    let rows: Vec<MediaPick> = c
        .model
        .library
        .iter()
        .filter(|m| m.enabled)
        .filter(|m| f.is_empty() || m.title.to_lowercase().contains(&f) || m.id.to_lowercase().contains(&f))
        .take(100)
        .map(|m| MediaPick {
            id: m.id.clone().into(),
            title: m.title.clone().into(),
            kind: crate::fmt::kind_id(m.kind).into(),
            duration: hms(m.play_duration()).into(),
        })
        .collect();
    a.set_media_results(model(rows));
}

/// Ad breaks and schedules for the selectors.
pub fn config_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let a = ui.global::<Air>();
    a.set_ad_breaks(strings(
        c.model.rules.ad_breaks.iter().map(|b| if b.name.is_empty() { b.id.clone() } else { format!("{} — {}", b.id, b.name) }),
    ));
    let mut sched = vec!["авто (по времени активации)".to_string()];
    sched.extend(
        c.model.schedules.schedules.iter().map(|s| if s.name.is_empty() { s.id.clone() } else { format!("{} — {}", s.id, s.name) }),
    );
    a.set_schedules(strings(sched));
    let idx = c
        .model
        .status
        .force_schedule
        .as_ref()
        .and_then(|f| c.model.schedules.schedules.iter().position(|s| &s.id == f))
        .map(|i| i as i32 + 1)
        .unwrap_or(0);
    a.set_schedule_index(idx);
}

/// Recent warnings / errors for the home screen.
pub fn log_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let rows: Vec<LogRow> = c
        .model
        .log
        .iter()
        .filter(|l| l.level == "warning" || l.level == "critical" || l.level == "error")
        .take(6)
        .map(|l| LogRow { at: l.at.clone().into(), level: l.level.clone().into(), text: l.text.clone().into() })
        .collect();
    ui.global::<Air>().set_recent_events(model(rows));
}

/// Restore the queue panel width.
pub fn apply_prefs(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    if let Some(w) = c.prefs.panels.get("air.queue") {
        ui.global::<Air>().set_side_width(*w);
    }
}

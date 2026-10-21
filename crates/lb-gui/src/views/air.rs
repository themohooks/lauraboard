//! On-air screen: preview, now / next, countdowns, transport and quick
//! controls.

use eframe::egui::{self, Color32, ProgressBar, RichText, Ui};
use serde_json::json;

use crate::app::{App, cmd};
use crate::widgets::{hms, kind_color, kind_name};

#[derive(Default)]
pub struct AirView {
    media_filter: String,
    picked_media: String,
    ad_break: String,
    ticker: String,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let mut v = std::mem::take(&mut app.views.air);
    ui.columns(2, |cols| {
        preview(app, &mut cols[0]);
        now_next(app, &mut cols[0]);
        transport(app, &mut cols[1]);
        cols[1].separator();
        quick(app, &mut cols[1], &mut v);
    });
    app.views.air = v;
}

fn preview(app: &App, ui: &mut Ui) {
    let w = ui.available_width();
    match &app.preview {
        Some(t) => {
            let size = t.size_vec2();
            let h = w * size.y / size.x.max(1.0);
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(w, h)));
        }
        None => {
            let (r, _) = ui.allocate_exact_size(egui::vec2(w, w * 9.0 / 16.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 4.0, Color32::from_gray(20));
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "нет превью", egui::FontId::proportional(20.0), Color32::GRAY);
        }
    }
}

fn now_next(app: &App, ui: &mut Ui) {
    let s = &app.model.status;
    ui.add_space(8.0);
    match s.on_air.as_ref().and_then(|o| o.item.as_ref().map(|i| (o, i))) {
        Some((o, it)) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("СЕЙЧАС").color(Color32::from_rgb(90, 220, 120)).strong());
                ui.colored_label(kind_color(it.kind), kind_name(it.kind));
                ui.label(RichText::new(&it.title).size(20.0).strong());
            });
            let frac = (o.elapsed_ms as f32 / it.duration_ms.max(1) as f32).clamp(0.0, 1.0);
            ui.add(ProgressBar::new(frac).text(format!(
                "{} / {}   осталось {}",
                hms(o.elapsed_ms),
                hms(it.duration_ms),
                hms(o.remaining_ms)
            )));
            ui.horizontal(|ui| {
                ui.label(format!("блок: {}", it.block_title.clone().unwrap_or_default()));
                let dev = o.deviation_ms / 1000;
                let col = if dev.abs() > 10 { Color32::from_rgb(255, 160, 60) } else { Color32::GRAY };
                ui.colored_label(col, format!("отклонение от плана: {dev:+} с"));
            });
        }
        None => {
            ui.label(RichText::new("Нет эфира").size(20.0));
        }
    }
    ui.add_space(6.0);
    if let Some(n) = &s.next {
        ui.horizontal(|ui| {
            ui.label(RichText::new("ДАЛЕЕ").strong());
            ui.colored_label(kind_color(n.kind), kind_name(n.kind));
            ui.label(RichText::new(&n.title).size(16.0));
            ui.label(format!("в {}", n.start.format("%H:%M:%S")));
        });
    }
    if let Some(p) = &s.next_program {
        ui.horizontal(|ui| {
            ui.label(RichText::new("СЛЕД. ПЕРЕДАЧА").strong());
            ui.label(format!("{} — {}", p.block_title.clone().unwrap_or_default(), p.title));
            ui.label(format!("в {} (через {})", p.start.format("%H:%M"), hms(s.time_to_next_program_ms.unwrap_or(0).max(0))));
        });
    }
    if let Some(e) = s.last_error.as_ref().or(s.output.last_error.as_ref()) {
        ui.colored_label(Color32::from_rgb(255, 120, 120), format!("Последняя ошибка: {e}"));
    }
    if !s.output.targets.is_empty() {
        ui.label(format!("Выходы: {}", s.output.targets.join(", ")));
    }
}

fn transport(app: &mut App, ui: &mut Ui) {
    let s = app.model.status.clone();
    ui.heading("Управление эфиром");
    ui.horizontal_wrapped(|ui| {
        let big = |t: &str| egui::Button::new(RichText::new(t).size(18.0)).min_size(egui::vec2(120.0, 40.0));
        if s.state == "stopped" {
            if ui.add(big("▶ Старт")).clicked() {
                app.command(cmd("start"));
            }
        } else if ui.add(big("■ Стоп")).clicked() {
            app.ask("Остановить вещание канала? Зрители увидят обрыв трансляции.", |a| a.command(cmd("stop")));
        }
        if ui.add(big("⏭ Пропустить")).clicked() {
            app.command(cmd("skip"));
        }
        if ui.add(big("↺ Повторить")).clicked() {
            app.command(cmd("restart"));
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if s.manual {
            if ui.button("▶ Вернуться к расписанию").clicked() {
                app.command(json!({ "cmd": "manual", "enabled": false }));
            }
        } else if ui.button("⏸ Приостановить расписание (ручной режим)").clicked() {
            app.command(json!({ "cmd": "manual", "enabled": true }));
        }
        ui.label(format!("в очереди: {}", s.manual_queue));
        if s.manual_queue > 0 && ui.button("очистить очередь").clicked() {
            app.command(cmd("clear_queue"));
        }
    });
    ui.horizontal(|ui| {
        let current = s.force_schedule.clone().unwrap_or_else(|| "авто".into());
        ui.label("Расписание:");
        egui::ComboBox::from_id_salt("sched").selected_text(current).show_ui(ui, |ui| {
            if ui.selectable_label(s.force_schedule.is_none(), "авто (по времени активации)").clicked() {
                app.command(json!({ "cmd": "switch_schedule", "schedule": null }));
            }
            for sc in app.model.schedules.schedules.clone() {
                if ui.selectable_label(s.force_schedule.as_deref() == Some(sc.id.as_str()), format!("{} — {}", sc.id, sc.name)).clicked()
                {
                    app.command(json!({ "cmd": "switch_schedule", "schedule": sc.id }));
                }
            }
        });
    });
}

fn quick(app: &mut App, ui: &mut Ui, v: &mut AirView) {
    ui.heading("Быстрые действия");
    ui.horizontal(|ui| {
        ui.label("Ролик:");
        ui.add(egui::TextEdit::singleline(&mut v.media_filter).hint_text("поиск").desired_width(140.0));
        egui::ComboBox::from_id_salt("pick").selected_text(&v.picked_media).width(220.0).show_ui(ui, |ui| {
            let f = v.media_filter.to_lowercase();
            for m in app.model.library.iter().filter(|m| f.is_empty() || m.title.to_lowercase().contains(&f) || m.id.contains(&f)).take(200)
            {
                ui.selectable_value(&mut v.picked_media, m.id.clone(), format!("{} ({})", m.title, hms(m.play_duration())));
            }
        });
    });
    ui.horizontal(|ui| {
        let ok = !v.picked_media.is_empty();
        if ui.add_enabled(ok, egui::Button::new("▶ Показать сейчас")).clicked() {
            let m = v.picked_media.clone();
            app.ask(format!("Прервать текущий ролик и показать '{m}'?"), move |a| {
                a.command(json!({ "cmd": "play_now", "media": m }))
            });
        }
        if ui.add_enabled(ok, egui::Button::new("＋ В очередь")).clicked() {
            app.command(json!({ "cmd": "queue", "media": v.picked_media }));
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Рекламный блок:");
        egui::ComboBox::from_id_salt("adb").selected_text(&v.ad_break).show_ui(ui, |ui| {
            for b in &app.model.rules.ad_breaks {
                ui.selectable_value(&mut v.ad_break, b.id.clone(), format!("{} — {}", b.id, b.name));
            }
        });
        let ok = !v.ad_break.is_empty();
        if ui.add_enabled(ok, egui::Button::new("следующим")).clicked() {
            app.command(json!({ "cmd": "ad_break", "ad_break": v.ad_break, "now": false }));
        }
        if ui.add_enabled(ok, egui::Button::new("сейчас")).clicked() {
            let b = v.ad_break.clone();
            app.ask("Прервать текущий ролик и запустить рекламу?", move |a| {
                a.command(json!({ "cmd": "ad_break", "ad_break": b, "now": true }))
            });
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Графика (ручное управление)").strong());
    let active: Vec<String> = app.model.status.graphics.iter().map(|g| g.element.clone()).collect();
    egui::Grid::new("gfx-quick").striped(true).show(ui, |ui| {
        for e in app.model.graphics.elements.clone() {
            let on = active.contains(&e.id);
            ui.colored_label(if on { Color32::from_rgb(90, 220, 120) } else { Color32::GRAY }, if on { "●" } else { "○" });
            ui.label(if e.name.is_empty() { &e.id } else { &e.name });
            if ui.small_button("показать").clicked() {
                app.command(json!({ "cmd": "graphic", "element": e.id, "action": "show" }));
            }
            if ui.small_button("скрыть").clicked() {
                app.command(json!({ "cmd": "graphic", "element": e.id, "action": "hide" }));
            }
            if ui.small_button("авто").clicked() {
                app.command(json!({ "cmd": "graphic_auto", "element": e.id }));
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Бегущая строка {ticker}:");
        ui.add(egui::TextEdit::singleline(&mut v.ticker).desired_width(260.0));
        if ui.button("применить").clicked() {
            let value = if v.ticker.trim().is_empty() { serde_json::Value::Null } else { json!(v.ticker) };
            app.command(json!({ "cmd": "set_var", "key": "ticker", "value": value }));
        }
    });
}

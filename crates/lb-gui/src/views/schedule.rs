//! Broadcast grid: day timeline, block editor, week mapping and a plan
//! preview for the next hours/days.

use chrono::{NaiveTime, Timelike};
use eframe::egui::{self, Color32, RichText, Ui};

use lb_core::schedule::{Block, BlockSource, LatePolicy, ScheduleSet};

use super::rundown::items_table;
use super::save_doc;
use crate::app::App;
use crate::net::Msg;

pub struct ScheduleView {
    draft: Option<ScheduleSet>,
    dirty: bool,
    sched: usize,
    tpl: usize,
    hours: i64,
    start_buf: Vec<String>,
}

impl Default for ScheduleView {
    fn default() -> Self {
        ScheduleView { draft: None, dirty: false, sched: 0, tpl: 0, hours: 12, start_buf: vec![] }
    }
}

pub fn load_plan(app: &App) {
    let ch = app.model.ch();
    let hours = app.views.schedule.hours;
    app.worker.spawn(move |api| Ok(Msg::Plan(api.get(&format!("/channels/{ch}/plan?hours={hours}"))?)));
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let mut v = std::mem::take(&mut app.views.schedule);
    if !v.dirty {
        v.draft = Some(app.model.schedules.clone());
    }
    egui::Panel::bottom("plan").resizable(true).default_size(300.0).show(ui, |ui| plan(app, ui, &mut v));
    egui::CentralPanel::default().show(ui, |ui| grid_editor(app, ui, &mut v));
    app.views.schedule = v;
}

fn plan(app: &mut App, ui: &mut Ui, v: &mut ScheduleView) {
    ui.horizontal(|ui| {
        ui.heading("Прогноз эфира");
        ui.add(egui::Slider::new(&mut v.hours, 1..=168).text("часов"));
        if ui.button("Рассчитать").clicked() {
            app.views.schedule.hours = v.hours;
            let ch = app.model.ch();
            let hours = v.hours;
            app.worker.spawn(move |api| Ok(Msg::Plan(api.get(&format!("/channels/{ch}/plan?hours={hours}"))?)));
        }
        let p = &app.model.plan;
        let gaps = p.items.iter().filter(|i| i.is_slate()).count();
        ui.label(format!("{} элементов, пустот: {gaps}, предупреждений: {}", p.items.len(), p.warnings.len()));
    });
    if !app.model.plan.warnings.is_empty() {
        egui::CollapsingHeader::new("Предупреждения (пустоты, пересечения, опоздания)").show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                for w in &app.model.plan.warnings {
                    ui.colored_label(
                        Color32::from_rgb(255, 200, 80),
                        format!("{} {}", w.at.map(|t| t.format("%d.%m %H:%M").to_string()).unwrap_or_default(), w.message),
                    );
                }
            });
        });
    }
    items_table(ui, &app.model.plan.items, "plan");
}

fn grid_editor(app: &mut App, ui: &mut Ui, v: &mut ScheduleView) {
    let playlists: Vec<String> = app.model.playlists.iter().map(|p| p.id.clone()).collect();
    let Some(set) = v.draft.as_mut() else { return };
    if set.schedules.is_empty() {
        ui.label("Нет расписаний. Создайте schedule.toml или импортируйте через CLI (`laurabrod schedule import`).");
        return;
    }
    v.sched = v.sched.min(set.schedules.len() - 1);
    ui.horizontal(|ui| {
        ui.heading("Сетка вещания");
        for (i, s) in set.schedules.iter().enumerate() {
            if ui.selectable_label(v.sched == i, format!("{} ({})", s.name, s.id)).clicked() {
                v.sched = i;
                v.tpl = 0;
            }
        }
        if ui.add_enabled(v.dirty, egui::Button::new(RichText::new("💾 Сохранить").strong())).clicked() {
            save_doc(app, "/schedules", set.clone(), "сетка");
            v.dirty = false;
        }
        if ui.add_enabled(v.dirty, egui::Button::new("Отменить")).clicked() {
            v.dirty = false;
        }
    });
    let s = &mut set.schedules[v.sched];
    ui.horizontal(|ui| {
        v.dirty |= ui.checkbox(&mut s.enabled, "активно").changed();
        ui.label(format!("активация: {}", s.activate_at.map(|t| t.to_string()).unwrap_or_else(|| "сразу".into())));
        ui.separator();
        ui.label("Неделя:");
        let w = &s.week;
        for (d, t) in [("пн", &w.mon), ("вт", &w.tue), ("ср", &w.wed), ("чт", &w.thu), ("пт", &w.fri), ("сб", &w.sat), ("вс", &w.sun)]
        {
            ui.label(format!("{d}={}", t.as_deref().or(w.default.as_deref()).unwrap_or("—")));
        }
    });
    if s.templates.is_empty() {
        return;
    }
    v.tpl = v.tpl.min(s.templates.len() - 1);
    ui.horizontal(|ui| {
        ui.label("Шаблон дня:");
        for (i, t) in s.templates.iter().enumerate() {
            if ui.selectable_label(v.tpl == i, &t.id).clicked() {
                v.tpl = i;
                v.start_buf.clear();
            }
        }
    });
    let tpl = &mut s.templates[v.tpl];
    timeline(ui, &tpl.blocks);
    if v.start_buf.len() != tpl.blocks.len() {
        v.start_buf = tpl.blocks.iter().map(|b| b.start.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default()).collect();
    }
    let mut remove = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("blocks").show(ui, |ui| {
        egui::Grid::new("blocks").striped(true).num_columns(7).show(ui, |ui| {
            for h in ["Начало (пусто = следом)", "Блок", "Название", "Источник", "При опоздании", "Обрезка разрешена", ""]
            {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (i, b) in tpl.blocks.iter_mut().enumerate() {
                let r = ui.add(egui::TextEdit::singleline(&mut v.start_buf[i]).desired_width(80.0));
                if r.changed() {
                    let t = v.start_buf[i].trim();
                    if t.is_empty() {
                        b.start = None;
                        v.dirty = true;
                    } else if let Ok(t) = NaiveTime::parse_from_str(t, "%H:%M:%S").or_else(|_| NaiveTime::parse_from_str(t, "%H:%M")) {
                        b.start = Some(t);
                        v.dirty = true;
                    }
                }
                ui.label(&b.id);
                v.dirty |= ui.text_edit_singleline(&mut b.title).changed();
                source_edit(ui, i, b, &playlists, &mut v.dirty);
                egui::ComboBox::from_id_salt(("late", i)).selected_text(late_name(b.late_policy)).show_ui(ui, |ui| {
                    for p in [LatePolicy::Strict, LatePolicy::Finish, LatePolicy::Truncate] {
                        v.dirty |= ui.selectable_value(&mut b.late_policy, p, late_name(p)).changed();
                    }
                });
                v.dirty |= ui.checkbox(&mut b.allow_truncate, "").changed();
                if ui.small_button("✖").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if ui.button("＋ Блок").clicked() {
            let n = tpl.blocks.len() + 1;
            let src = BlockSource::Playlist { playlist: playlists.first().cloned().unwrap_or_default(), restart: false };
            tpl.blocks.push(Block::new(&format!("block-{n}"), None, src));
            v.start_buf.clear();
            v.dirty = true;
        }
    });
    if let Some(i) = remove {
        tpl.blocks.remove(i);
        v.start_buf.clear();
        v.dirty = true;
    }
}

fn late_name(p: LatePolicy) -> &'static str {
    match p {
        LatePolicy::Strict => "не начинать / ужать вставки",
        LatePolicy::Finish => "доиграть, сдвинуть след.",
        LatePolicy::Truncate => "обрезать (если разрешено)",
    }
}

fn source_edit(ui: &mut Ui, i: usize, b: &mut Block, playlists: &[String], dirty: &mut bool) {
    match &mut b.source {
        BlockSource::Playlist { playlist, .. } => {
            egui::ComboBox::from_id_salt(("src", i)).selected_text(format!("плейлист: {playlist}")).show_ui(ui, |ui| {
                for p in playlists {
                    *dirty |= ui.selectable_value(playlist, p.clone(), p).changed();
                }
            });
        }
        BlockSource::Media { items } => {
            ui.label(format!("ролики: {}", items.join(", ")));
        }
        BlockSource::Category { category, .. } => {
            ui.label("категория:");
            *dirty |= ui.text_edit_singleline(category).changed();
        }
        BlockSource::AdBreak { ad_break } => {
            ui.label(format!("реклама: {ad_break}"));
        }
    }
}

/// 24-hour bar with fixed block starts.
fn timeline(ui: &mut Ui, blocks: &[Block]) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 46.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, Color32::from_gray(30));
    let x_of = |secs: u32| rect.left() + rect.width() * secs as f32 / 86_400.0;
    for h in (0..=24).step_by(3) {
        let x = x_of(h * 3600);
        p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], (1.0, Color32::from_gray(60)));
        p.text(
            egui::pos2(x + 2.0, rect.bottom() - 12.0),
            egui::Align2::LEFT_TOP,
            format!("{h:02}"),
            egui::FontId::monospace(10.0),
            Color32::GRAY,
        );
    }
    let fixed: Vec<(u32, &Block)> = blocks.iter().filter_map(|b| b.start.map(|s| (s.num_seconds_from_midnight(), b))).collect();
    let palette = [
        Color32::from_rgb(70, 120, 200),
        Color32::from_rgb(90, 170, 120),
        Color32::from_rgb(180, 120, 200),
        Color32::from_rgb(200, 150, 70),
    ];
    for (i, (s, b)) in fixed.iter().enumerate() {
        let e = fixed.get(i + 1).map(|x| x.0).unwrap_or(86_400);
        let r =
            egui::Rect::from_min_max(egui::pos2(x_of(*s), rect.top() + 4.0), egui::pos2(x_of(e).max(x_of(*s) + 2.0), rect.top() + 30.0));
        p.rect_filled(r, 2.0, palette[i % palette.len()]);
        if r.width() > 40.0 {
            p.text(
                r.left_center() + egui::vec2(3.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &b.title,
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
    }
}

//! On-screen graphics: live manual control, display rules and default
//! template variables.

use eframe::egui::{self, Color32, RichText, Ui};
use serde_json::json;

use lb_core::graphics::{ElementKind, GraphicsConfig};

use super::save_doc;
use crate::app::App;
use crate::widgets::dur_edit;

#[derive(Default)]
pub struct GraphicsView {
    draft: Option<GraphicsConfig>,
    dirty: bool,
    bufs: Vec<[String; 4]>,
    new_var: (String, String),
}

fn kind_label(k: &ElementKind) -> &'static str {
    match k {
        ElementKind::Image { .. } => "изображение",
        ElementKind::Text { .. } => "текст",
        ElementKind::Ticker { .. } => "бегущая строка",
        ElementKind::Clock { .. } => "часы",
        ElementKind::Countdown { .. } => "таймер",
        ElementKind::Rect { .. } => "плашка",
        ElementKind::Group { .. } => "композиция",
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let mut v = std::mem::take(&mut app.views.graphics);
    if !v.dirty {
        v.draft = Some(app.model.graphics.clone());
        v.bufs.clear();
    }
    let active: Vec<String> = app.model.status.graphics.iter().map(|g| g.element.clone()).collect();
    let Some(g) = v.draft.as_mut() else { return };
    ui.horizontal(|ui| {
        ui.heading("Экранная графика");
        if ui.add_enabled(v.dirty, egui::Button::new(RichText::new("💾 Сохранить").strong())).clicked() {
            save_doc(app, "/graphics", g.clone(), "графика");
            v.dirty = false;
        }
        if ui.add_enabled(v.dirty, egui::Button::new("Отменить")).clicked() {
            v.dirty = false;
        }
    });
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        ui.label(RichText::new("Элементы (живое управление)").strong());
        egui::Grid::new("els").striped(true).show(ui, |ui| {
            for e in &g.elements {
                let on = active.contains(&e.id);
                ui.colored_label(if on { Color32::from_rgb(90, 220, 120) } else { Color32::GRAY }, if on { "● в эфире" } else { "○" });
                ui.label(&e.id);
                ui.label(kind_label(&e.kind));
                ui.label(format!("слой {}", e.layer));
                if ui.small_button("показать").clicked() {
                    app.command(json!({ "cmd": "graphic", "element": e.id, "action": "show" }));
                }
                if ui.small_button("скрыть").clicked() {
                    app.command(json!({ "cmd": "graphic", "element": e.id, "action": "hide" }));
                }
                if ui.small_button("по правилам").clicked() {
                    app.command(json!({ "cmd": "graphic_auto", "element": e.id }));
                }
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Переменные шаблонов по умолчанию").strong());
        let mut remove = None;
        for (k, val) in g.vars.iter_mut() {
            ui.horizontal(|ui| {
                ui.label(format!("{{{k}}}"));
                v.dirty |= ui.text_edit_singleline(val).changed();
                if ui.small_button("✖").clicked() {
                    remove = Some(k.clone());
                }
            });
        }
        if let Some(k) = remove {
            g.vars.remove(&k);
            v.dirty = true;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut v.new_var.0).hint_text("имя").desired_width(80.0));
            ui.add(egui::TextEdit::singleline(&mut v.new_var.1).hint_text("значение"));
            if ui.button("＋").clicked() && !v.new_var.0.trim().is_empty() {
                g.vars.insert(v.new_var.0.trim().to_string(), v.new_var.1.clone());
                v.new_var = Default::default();
                v.dirty = true;
            }
        });
        ui.label(RichText::new("Встроенные: {title} {category} {block} {presenter} {next_title} {next_start} {next_program} {next_program_start} {channel}").weak());

        let ui = &mut cols[1];
        ui.label(RichText::new("Правила показа").strong());
        if v.bufs.len() != g.rules.len() {
            v.bufs = vec![Default::default(); g.rules.len()];
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, r) in g.rules.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        v.dirty |= ui.checkbox(&mut r.enabled, "").changed();
                        ui.label(RichText::new(&r.id).strong());
                        ui.label(format!("→ {}", r.element));
                        ui.label("приоритет");
                        v.dirty |= ui.add(egui::DragValue::new(&mut r.priority)).changed();
                    });
                    ui.horizontal(|ui| {
                        let b = &mut v.bufs[i];
                        let t = &mut r.timing;
                        let before = (t.after_start, t.duration, t.before_end, t.last);
                        dur_edit(ui, "через", &mut t.after_start, &mut b[0]);
                        dur_edit(ui, "на", &mut t.duration, &mut b[1]);
                        dur_edit(ui, "скрыть до конца за", &mut t.before_end, &mut b[2]);
                        dur_edit(ui, "последние", &mut t.last, &mut b[3]);
                        v.dirty |= before != (t.after_start, t.duration, t.before_end, t.last);
                    });
                    let c = &r.conditions;
                    let mut cond = Vec::new();
                    if !c.kinds.is_empty() {
                        cond.push(format!("типы: {:?}", c.kinds));
                    }
                    if !c.categories.is_empty() {
                        cond.push(format!("категории: {}", c.categories.join(",")));
                    }
                    if !c.exclude_categories.is_empty() {
                        cond.push(format!("кроме: {}", c.exclude_categories.join(",")));
                    }
                    if !r.timing.at.is_empty() {
                        cond.push(format!("в {:?}", r.timing.at));
                    }
                    if !cond.is_empty() {
                        ui.label(RichText::new(cond.join("; ")).weak());
                    }
                });
            }
        });
    });
    app.views.graphics = v;
}

//! Rundown: upcoming items with estimated start/end and deviations.

use eframe::egui::{self, Color32, RichText, Ui};

use lb_core::rundown::Origin;

use crate::app::App;
use crate::widgets::{hms, kind_color, kind_name};

pub fn show(app: &mut App, ui: &mut Ui) {
    let r = &app.model.rundown;
    ui.horizontal(|ui| {
        ui.heading("Рундаун");
        ui.label(format!("{} элементов, итого {}", r.items.len(), hms(r.total_ms())));
    });
    if !r.warnings.is_empty() {
        egui::CollapsingHeader::new(
            RichText::new(format!("⚠ Предупреждения ({})", r.warnings.len())).color(Color32::from_rgb(255, 200, 80)),
        )
        .show(ui, |ui| {
            for w in &r.warnings {
                ui.label(format!("{} — {}", w.at.map(|t| t.format("%d.%m %H:%M:%S").to_string()).unwrap_or_default(), w.message));
            }
        });
    }
    items_table(ui, &app.model.rundown.items, "rundown");
}

/// Shared table for rundown / plan items.
pub fn items_table(ui: &mut Ui, items: &[lb_core::rundown::RundownItem], id: &str) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(id).show(ui, |ui| {
        egui::Grid::new(id).striped(true).num_columns(7).show(ui, |ui| {
            for h in ["Начало", "Конец", "Длит.", "Тип", "Название", "Блок", "Примечание"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            let mut last_block = None;
            for it in items {
                let new_block = it.block_key != last_block;
                last_block = it.block_key.clone();
                let start = it.start.format("%H:%M:%S").to_string();
                ui.label(if new_block { RichText::new(start).strong() } else { RichText::new(start) });
                ui.label(it.end().format("%H:%M:%S").to_string());
                ui.label(hms(it.duration_ms));
                if it.is_slate() {
                    ui.colored_label(Color32::from_rgb(255, 120, 120), "пустота");
                } else {
                    ui.colored_label(kind_color(it.kind), kind_name(it.kind));
                }
                let title = if it.origin == Origin::Main { RichText::new(&it.title).strong() } else { RichText::new(&it.title) };
                ui.label(title);
                ui.label(it.block_title.clone().unwrap_or_default());
                let mut note = Vec::new();
                if let Some(d) = it.fixed_deviation_ms() {
                    note.push(if d == 0 { "фикс. время".to_string() } else { format!("фикс. {:+} с", d / 1000) });
                }
                if it.truncated {
                    note.push("обрезано".into());
                }
                if it.optional && it.origin != Origin::Main {
                    note.push("необязательный".into());
                }
                if let Some(r) = &it.rule {
                    note.push(format!("правило {r}"));
                }
                ui.label(note.join(", "));
                ui.end_row();
            }
        });
    });
}

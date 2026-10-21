//! Interface settings and connection info.

use eframe::egui::{self, Ui};

use crate::app::App;

pub fn show(app: &mut App, ui: &mut Ui) {
    ui.heading("Настройки интерфейса");
    ui.horizontal(|ui| {
        ui.label("Тема:");
        if ui.selectable_label(app.dark, "тёмная").clicked() {
            app.dark = true;
            ui.ctx().set_visuals(egui::Visuals::dark());
        }
        if ui.selectable_label(!app.dark, "светлая").clicked() {
            app.dark = false;
            ui.ctx().set_visuals(egui::Visuals::light());
        }
    });
    ui.horizontal(|ui| {
        ui.label("Масштаб:");
        let mut z = ui.ctx().zoom_factor();
        if ui.add(egui::Slider::new(&mut z, 0.7..=1.8)).changed() {
            ui.ctx().set_zoom_factor(z);
        }
    });
    ui.separator();
    ui.label(format!("Сервер API: {}", app.worker.api().base));
    ui.label(format!("Связь: {}", if app.model.online { "есть" } else { "нет" }));
    ui.label(format!("Каналы: {}", app.model.channels.join(", ")));
    ui.add_space(8.0);
    ui.label(
        "Параметры канала (выходы, кодирование, ключи трансляции) редактируются в channels/<id>/channel.toml или через API (роль admin).",
    );
}

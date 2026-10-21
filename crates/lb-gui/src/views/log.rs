//! Real-time event log (WebSocket).

use eframe::egui::{self, Color32, RichText, Ui};

use crate::app::App;

pub fn show(app: &mut App, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.heading("Журнал событий");
        if ui.button("Очистить").clicked() {
            app.model.log.clear();
        }
    });
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for l in &app.model.log {
            let col = match l.level.as_str() {
                "critical" => Color32::from_rgb(255, 100, 100),
                "warning" => Color32::from_rgb(255, 200, 80),
                _ => ui.visuals().text_color(),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(&l.at).monospace().weak());
                ui.colored_label(col, &l.text);
            });
        }
    });
}

//! Statistics for a period, ad / promo airings, CSV export.

use eframe::egui::{self, RichText, Ui};

use crate::app::App;
use crate::net::Msg;
use crate::widgets::hms;

pub struct ReportsView {
    from: String,
    to: String,
    export_path: String,
}

impl Default for ReportsView {
    fn default() -> Self {
        let today = chrono::Local::now().date_naive().to_string();
        ReportsView { from: today.clone(), to: today, export_path: String::new() }
    }
}

pub fn load(app: &App) {
    let v = &app.views.reports;
    let (ch, from, to) = (app.model.ch(), v.from.clone(), v.to.clone());
    app.worker.spawn(move |api| Ok(Msg::Stats(api.get(&format!("/channels/{ch}/stats?from={from}&to={to}"))?)));
}

fn table(ui: &mut Ui, id: &str, rows: &serde_json::Value) {
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for h in ["ролик", "показов", "пропущено", "в эфире", "название"] {
            ui.label(RichText::new(h).strong());
        }
        ui.end_row();
        for r in rows.as_array().cloned().unwrap_or_default() {
            ui.label(r["media"].as_str().unwrap_or(""));
            ui.label(r["plays"].to_string());
            ui.label(r["skipped"].to_string());
            ui.label(hms(r["airtime_ms"].as_i64().unwrap_or(0)));
            ui.label(r["title"].as_str().unwrap_or(""));
            ui.end_row();
        }
    });
}

pub fn show(app: &mut App, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.heading("Статистика и отчёты");
        let v = &mut app.views.reports;
        ui.label("с");
        ui.add(egui::TextEdit::singleline(&mut v.from).desired_width(90.0));
        ui.label("по");
        ui.add(egui::TextEdit::singleline(&mut v.to).desired_width(90.0));
    });
    if ui.button("Показать").clicked() {
        load(app);
    }
    ui.horizontal(|ui| {
        ui.label("Экспорт CSV в файл:");
        ui.add(egui::TextEdit::singleline(&mut app.views.reports.export_path).hint_text("C:/reports/asrun.csv").desired_width(300.0));
        for (kind, label) in [("asrun", "журнал эфира"), ("stats", "статистика")] {
            if ui.button(label).clicked() {
                let v = &app.views.reports;
                let (ch, from, to, path) = (app.model.ch(), v.from.clone(), v.to.clone(), v.export_path.clone());
                if path.trim().is_empty() {
                    app.toast("Укажите путь для сохранения", true);
                    continue;
                }
                app.worker.spawn(move |api| {
                    let bytes = api.get_bytes(&format!("/channels/{ch}/{kind}?from={from}&to={to}&format=csv"))?;
                    std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))?;
                    Ok(Msg::Done(format!("сохранено: {path}")))
                });
            }
        }
    });
    let s = app.model.stats.clone();
    if s.is_null() {
        return;
    }
    ui.separator();
    ui.label(format!(
        "Элементов: {}   в эфире: {}   пропущено: {}   прервано: {}   среднее отклонение: {:.1} с   максимальное: {:.1} с",
        s["items"],
        hms(s["airtime_ms"].as_i64().unwrap_or(0)),
        s["skipped"],
        s["interrupted"],
        s["avg_deviation_ms"].as_f64().unwrap_or(0.0) / 1000.0,
        s["max_deviation_ms"].as_f64().unwrap_or(0.0) / 1000.0
    ));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::CollapsingHeader::new("Реклама").default_open(true).show(ui, |ui| table(ui, "ads", &s["ads"]));
        egui::CollapsingHeader::new("Анонсы").default_open(true).show(ui, |ui| table(ui, "promos", &s["promos"]));
        egui::CollapsingHeader::new("Все ролики").show(ui, |ui| table(ui, "all", &s["by_media"]));
    });
}

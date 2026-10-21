//! Media library: browse, import by path, edit metadata, remove.

use eframe::egui::{self, Color32, RichText, Ui};
use serde_json::json;

use lb_core::media::{MediaItem, MediaKind};

use crate::app::App;
use crate::net::Msg;
use crate::widgets::{KINDS, hms, kind_color, kind_name};

#[derive(Default)]
pub struct LibraryView {
    filter: String,
    kind: Option<MediaKind>,
    import_path: String,
    import_kind: Option<MediaKind>,
    import_category: String,
    edit: Option<MediaItem>,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let mut v = std::mem::take(&mut app.views.library);
    ui.heading("Медиатека");
    import_bar(app, ui, &mut v);
    ui.separator();
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut v.filter).hint_text("поиск").desired_width(200.0));
        egui::ComboBox::from_id_salt("lib-kind").selected_text(v.kind.map(kind_name).unwrap_or("все типы")).show_ui(ui, |ui| {
            ui.selectable_value(&mut v.kind, None, "все типы");
            for k in KINDS {
                ui.selectable_value(&mut v.kind, Some(k), kind_name(k));
            }
        });
        if ui.button("Проверить файлы").clicked() {
            let ch = app.model.ch();
            app.worker.spawn(move |api| {
                let r = api.post(&format!("/channels/{ch}/media/check"), &json!({}))?;
                let n = r["problems"].as_array().map(|a| a.len()).unwrap_or(0);
                Ok(Msg::Done(if n == 0 {
                    "все файлы в порядке".into()
                } else {
                    format!("проблемных файлов: {n} (см. журнал)")
                }))
            });
            app.load_all();
        }
    });
    let f = v.filter.to_lowercase();
    let items: Vec<MediaItem> = app
        .model
        .library
        .iter()
        .filter(|m| v.kind.is_none_or(|k| m.kind == k))
        .filter(|m| {
            f.is_empty() || m.title.to_lowercase().contains(&f) || m.id.contains(&f) || m.category.as_deref().unwrap_or("").contains(&f)
        })
        .cloned()
        .collect();
    ui.label(format!("{} элементов, {}", items.len(), hms(items.iter().map(|m| m.play_duration()).sum())));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - if v.edit.is_some() { 170.0 } else { 0.0 })
        .show(ui, |ui| {
            egui::Grid::new("lib").striped(true).num_columns(7).show(ui, |ui| {
                for h in ["id", "тип", "длит.", "категория", "название", "состояние", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for m in &items {
                    ui.label(&m.id);
                    ui.colored_label(kind_color(m.kind), kind_name(m.kind));
                    ui.label(hms(m.play_duration()));
                    ui.label(m.category.clone().unwrap_or_default());
                    ui.label(&m.title);
                    match &m.probe.error {
                        Some(e) => ui.colored_label(Color32::from_rgb(255, 120, 120), format!("ошибка: {e}")),
                        None => ui
                            .label(m.probe.video.as_ref().map(|v| format!("{}x{} {:.2}fps", v.width, v.height, v.fps)).unwrap_or_default()),
                    };
                    ui.horizontal(|ui| {
                        if ui.small_button("изменить").clicked() {
                            v.edit = Some(m.clone());
                        }
                        if ui.small_button("удалить").clicked() {
                            let id = m.id.clone();
                            app.ask(
                                format!("Удалить '{id}' из медиатеки? (файл на диске останется)"),
                                move |a| {
                                    let ch = a.model.ch();
                                    a.worker.spawn(move |api| {
                                        api.delete(&format!("/channels/{ch}/media/{id}"))?;
                                        Ok(Msg::Done("удалено".into()))
                                    });
                                    a.load_all();
                                },
                            );
                        }
                    });
                    ui.end_row();
                }
            });
        });
    edit_panel(app, ui, &mut v);
    app.views.library = v;
}

fn import_bar(app: &mut App, ui: &mut Ui, v: &mut LibraryView) {
    ui.horizontal(|ui| {
        ui.label("Добавить:");
        ui.add(egui::TextEdit::singleline(&mut v.import_path).hint_text("путь к файлу, папке или URL на сервере").desired_width(380.0));
        egui::ComboBox::from_id_salt("imp-kind").selected_text(v.import_kind.map(kind_name).unwrap_or("авто")).show_ui(ui, |ui| {
            ui.selectable_value(&mut v.import_kind, None, "авто");
            for k in KINDS {
                ui.selectable_value(&mut v.import_kind, Some(k), kind_name(k));
            }
        });
        ui.add(egui::TextEdit::singleline(&mut v.import_category).hint_text("категория").desired_width(120.0));
        if ui.add_enabled(!v.import_path.trim().is_empty(), egui::Button::new("Импорт")).clicked() {
            let body = json!({
                "paths": [v.import_path.trim()],
                "kind": v.import_kind,
                "category": if v.import_category.trim().is_empty() { None } else { Some(v.import_category.trim()) },
                "recursive": true
            });
            let ch = app.model.ch();
            app.worker.spawn(move |api| {
                let r = api.post(&format!("/channels/{ch}/media/import"), &body)?;
                let added = r["added"].as_array().map(|a| a.len()).unwrap_or(0);
                let errors: Vec<String> =
                    r["errors"].as_array().map(|a| a.iter().filter_map(|e| e.as_str().map(str::to_string)).collect()).unwrap_or_default();
                if added == 0 && !errors.is_empty() {
                    return Err(errors.join("; "));
                }
                Ok(Msg::Done(format!(
                    "добавлено: {added}{}",
                    if errors.is_empty() { String::new() } else { format!(", ошибок: {}", errors.len()) }
                )))
            });
            v.import_path.clear();
            app.load_all();
        }
    });
}

fn edit_panel(app: &mut App, ui: &mut Ui, v: &mut LibraryView) {
    let Some(m) = v.edit.as_mut() else { return };
    ui.separator();
    ui.label(RichText::new(format!("Редактирование: {}", m.id)).strong());
    egui::Grid::new("lib-edit").num_columns(4).show(ui, |ui| {
        ui.label("Название");
        ui.text_edit_singleline(&mut m.title);
        ui.label("Тип");
        egui::ComboBox::from_id_salt("edit-kind").selected_text(kind_name(m.kind)).show_ui(ui, |ui| {
            for k in KINDS {
                ui.selectable_value(&mut m.kind, k, kind_name(k));
            }
        });
        ui.end_row();
        ui.label("Категория");
        let mut cat = m.category.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut cat).changed() {
            m.category = Some(cat).filter(|c| !c.trim().is_empty());
        }
        ui.label("Громкость, дБ");
        ui.add(egui::DragValue::new(&mut m.volume_db).range(-30.0..=20.0).speed(0.5));
        ui.end_row();
        ui.label("Вес (случайный выбор)");
        ui.add(egui::DragValue::new(&mut m.weight).range(0.0..=100.0).speed(0.1));
        ui.checkbox(&mut m.enabled, "включён");
        ui.end_row();
    });
    let (mut save, mut close) = (false, false);
    ui.horizontal(|ui| {
        save = ui.button("💾 Сохранить").clicked();
        close = ui.button("Закрыть").clicked();
    });
    if save {
        let item = m.clone();
        let ch = app.model.ch();
        app.worker.spawn(move |api| {
            api.put(&format!("/channels/{ch}/media/{}", item.id), &item)?;
            Ok(Msg::Done("сохранено".into()))
        });
        app.load_all();
    }
    if save || close {
        v.edit = None;
    }
}

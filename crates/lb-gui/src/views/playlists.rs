//! Playlist editor: drag & drop ordering, pinning, loop/shuffle, totals.

use eframe::egui::{self, Color32, RichText, Ui};

use lb_core::playlist::{Playlist, PlaylistEntry};

use super::save_doc;
use crate::app::App;
use crate::net::Msg;
use crate::widgets::{hms, kind_color, kind_name};

#[derive(Default)]
pub struct PlaylistsView {
    selected: Option<String>,
    draft: Option<Playlist>,
    dirty: bool,
    filter: String,
    new_id: String,
}

impl PlaylistsView {
    /// Fresh data from the server: replace the draft unless it was edited.
    pub fn on_reload(&mut self, lists: &[Playlist]) {
        if self.dirty {
            return;
        }
        self.draft = self.selected.as_ref().and_then(|id| lists.iter().find(|p| &p.id == id).cloned());
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let mut v = std::mem::take(&mut app.views.playlists);
    egui::Panel::left("pl-list").resizable(true).default_size(240.0).show(ui, |ui| list(app, ui, &mut v));
    egui::Panel::right("pl-lib").resizable(true).default_size(320.0).show(ui, |ui| library(app, ui, &mut v));
    egui::CentralPanel::default().show(ui, |ui| editor(app, ui, &mut v));
    app.views.playlists = v;
}

fn list(app: &mut App, ui: &mut Ui, v: &mut PlaylistsView) {
    ui.heading("Плейлисты");
    for p in app.model.playlists.clone() {
        let sel = v.selected.as_deref() == Some(p.id.as_str());
        let label = format!(
            "{} ({}, {})",
            p.name,
            p.entries.len(),
            hms(p.total_duration(&lb_core::media::Library::new(app.model.library.clone())))
        );
        if ui.selectable_label(sel, label).clicked() && !sel {
            if v.dirty {
                app.toast("Есть несохранённые изменения — сохраните или отмените", true);
            } else {
                v.selected = Some(p.id.clone());
                v.draft = Some(p);
            }
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut v.new_id).hint_text("id нового").desired_width(120.0));
        if ui.add_enabled(!v.new_id.trim().is_empty(), egui::Button::new("Создать")).clicked() {
            let p = Playlist::new(v.new_id.trim());
            let ch = app.model.ch();
            app.worker.spawn(move |api| {
                api.post(&format!("/channels/{ch}/playlists"), &p)?;
                Ok(Msg::Done("плейлист создан".into()))
            });
            v.selected = Some(v.new_id.trim().to_string());
            v.new_id.clear();
            app.load_all();
        }
    });
}

fn editor(app: &mut App, ui: &mut Ui, v: &mut PlaylistsView) {
    let lib = lb_core::media::Library::new(app.model.library.clone());
    let Some(mut draft) = v.draft.take() else {
        ui.label("Выберите плейлист слева");
        return;
    };
    let p = &mut draft;
    let (mut reset, mut deleted) = (false, false);
    ui.horizontal(|ui| {
        ui.heading(&p.id);
        ui.label("Название:");
        v.dirty |= ui.text_edit_singleline(&mut p.name).changed();
        v.dirty |= ui.checkbox(&mut p.looped, "по кругу").changed();
        v.dirty |= ui.checkbox(&mut p.shuffle, "случайный порядок").changed();
    });
    ui.label(format!("Итого: {} элементов, {}", p.entries.len(), hms(p.total_duration(&lib))));
    ui.horizontal(|ui| {
        if ui.add_enabled(v.dirty, egui::Button::new(RichText::new("💾 Сохранить").strong())).clicked() {
            save_doc(app, &format!("/playlists/{}", p.id), p.clone(), "плейлист");
            v.dirty = false;
        }
        if ui.add_enabled(v.dirty, egui::Button::new("Отменить изменения")).clicked() {
            v.dirty = false;
            reset = true;
        }
        if ui.button("Очистить").clicked() && !p.entries.is_empty() {
            let id = p.id.clone();
            let mut cleared = p.clone();
            cleared.entries.clear();
            app.ask(format!("Удалить все элементы из плейлиста '{id}'?"), move |a| {
                save_doc(a, &format!("/playlists/{id}"), cleared, "плейлист очищен")
            });
        }
        if ui.button(RichText::new("Удалить плейлист").color(Color32::from_rgb(255, 120, 120))).clicked() {
            let id = p.id.clone();
            v.selected = None;
            v.dirty = false;
            deleted = true;
            app.ask(format!("Удалить плейлист '{id}'?"), move |a| {
                let ch = a.model.ch();
                a.worker.spawn(move |api| {
                    api.delete(&format!("/channels/{ch}/playlists/{id}"))?;
                    Ok(Msg::Done("плейлист удалён".into()))
                });
                a.load_all();
            });
        }
    });
    ui.separator();
    ui.label(RichText::new("Перетаскивайте строки за ☰, чтобы изменить порядок").weak());
    let mut from_to: Option<(usize, usize)> = None;
    let mut remove: Option<usize> = None;
    let mut offset = 0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (i, e) in p.entries.iter_mut().enumerate() {
            let m = lib.get(&e.media);
            let d = m.map(|m| m.play_duration()).unwrap_or(0);
            let row_id = egui::Id::unique(("pl-row", i));
            let resp = ui
                .dnd_drop_zone::<usize, _>(egui::Frame::new().inner_margin(2), |ui| {
                    ui.horizontal(|ui| {
                        ui.dnd_drag_source(row_id, i, |ui| {
                            ui.label("☰");
                        });
                        ui.label(format!("{:>3}", i + 1));
                        ui.label(RichText::new(format!("+{}", hms(offset))).monospace().weak());
                        match m {
                            Some(m) => {
                                ui.colored_label(kind_color(m.kind), kind_name(m.kind));
                                ui.label(&m.title);
                            }
                            None => {
                                ui.colored_label(Color32::RED, format!("нет в медиатеке: {}", e.media));
                            }
                        }
                        ui.label(hms(d));
                        if ui.checkbox(&mut e.pinned, "закреплён").changed() {
                            from_to = from_to.or(Some((usize::MAX, usize::MAX)));
                        }
                        if ui.checkbox(&mut e.disabled, "выкл").changed() {
                            from_to = from_to.or(Some((usize::MAX, usize::MAX)));
                        }
                        if ui.small_button("✖").clicked() {
                            remove = Some(i);
                        }
                    });
                })
                .1;
            if let Some(src) = resp {
                from_to = Some((*src, i));
            }
            if !e.disabled {
                offset += d;
            }
        }
    });
    match from_to {
        Some((usize::MAX, _)) => v.dirty = true,
        Some((a, b)) if a != b => {
            p.move_entry(a, b);
            v.dirty = true;
        }
        _ => {}
    }
    if let Some(i) = remove {
        p.entries.remove(i);
        v.dirty = true;
    }
    if deleted {
        v.draft = None;
    } else if reset {
        v.on_reload(&app.model.playlists);
    } else {
        v.draft = Some(draft);
    }
}

fn library(app: &mut App, ui: &mut Ui, v: &mut PlaylistsView) {
    ui.heading("Добавить из медиатеки");
    ui.add(egui::TextEdit::singleline(&mut v.filter).hint_text("поиск"));
    let f = v.filter.to_lowercase();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for m in app.model.library.iter().filter(|m| f.is_empty() || m.title.to_lowercase().contains(&f) || m.id.contains(&f)) {
            ui.horizontal(|ui| {
                if ui.add_enabled(v.draft.is_some(), egui::Button::new("＋")).clicked()
                    && let Some(p) = v.draft.as_mut()
                {
                    p.entries.push(PlaylistEntry::new(m.id.clone()));
                    v.dirty = true;
                }
                ui.colored_label(kind_color(m.kind), kind_name(m.kind));
                ui.label(format!("{} ({})", m.title, hms(m.play_duration())));
            });
        }
    });
}

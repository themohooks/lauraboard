//! Media library: filtered / sorted table and tiles, multi-select with
//! bulk actions, metadata editing, import (copying into the channel's
//! storage as a background job) and file check. Folders: [`super::folders`].

use std::collections::HashSet;
use std::rc::Rc;

use serde_json::{Value, json};
use slint::{ComponentHandle, Model, SharedString, VecModel};

use lb_core::media::{MediaItem, MediaKind};
use lb_core::playlist::PlaylistEntry;

use super::{Ctx, thumbs, vec_model, with};
use crate::fmt::{KINDS, hms, kind_id};
use crate::logic::filter::{self, MediaFilter, SortKey};
use crate::net::Msg;
use crate::shell::{AppWindow, Lib, MediaEdit, MediaRow, TileLine};
use super::folders;

#[derive(Default)]
pub struct LibraryState {
    pub loaded: bool,
    /// Library indices in display order.
    shown: Vec<usize>,
    pub checked: HashSet<String>,
    columns: usize,
    rows: Rc<VecModel<MediaRow>>,
    categories: Vec<String>,
    edit_id: Option<String>,
    /// Explicit folders from the server (empty ones included).
    pub folders: Vec<String>,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Lib>();
    g.on_refresh_list(|| with(refresh));
    g.on_sort(|k| {
        with(move |c| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Lib>();
            if g.get_sort_key() == k {
                g.set_sort_desc(!g.get_sort_desc());
            } else {
                g.set_sort_key(k);
                g.set_sort_desc(false);
            }
            refresh(c);
        })
    });
    g.on_columns_changed(|n| {
        with(move |c| {
            if c.lib.columns != n as usize {
                c.lib.columns = n.max(1) as usize;
                lines(c);
            }
        })
    });
    g.on_toggle_check(|row| {
        with(move |c| {
            let Some(id) = id_at(c, row) else { return };
            if !c.lib.checked.remove(&id) {
                c.lib.checked.insert(id);
            }
            update_checks(c);
        })
    });
    g.on_check_all(|on| {
        with(move |c| {
            if on {
                c.lib.checked = c.lib.shown.iter().map(|&i| c.model.library[i].id.clone()).collect();
            } else {
                c.lib.checked.clear();
            }
            update_checks(c);
        })
    });
    g.on_open_edit(|row| with(move |c| open_edit(c, row)));
    g.on_validate_edit(|| with(validate_edit));
    g.on_save_edit(|| with(save_edit));
    g.on_row_action(|row, a| with(move |c| row_action(c, row, a.as_str())));
    g.on_bulk(|a, arg| with(move |c| bulk(c, a.as_str(), arg)));
    g.on_browse_import(|folder| {
        with(move |c| {
            let d = rfd::FileDialog::new();
            let picked: Vec<std::path::PathBuf> = if folder {
                d.pick_folder().into_iter().collect()
            } else {
                d.set_title("Выберите ролики")
                    .add_filter("Видео и аудио", MEDIA_EXT)
                    .add_filter("Изображения", &["png", "jpg", "jpeg", "webp", "bmp"])
                    .add_filter("Все файлы", &["*"])
                    .pick_files()
                    .unwrap_or_default()
            };
            if let (false, Some(ui)) = (picked.is_empty(), c.window()) {
                let list: Vec<String> = picked.iter().map(|p| p.display().to_string()).collect();
                let g = ui.global::<Lib>();
                g.set_import_path(list.join(PATH_SEP).into());
                // opened from the toolbar: continue in the dialog (kind, category)
                g.set_import_open(true);
            }
        })
    });
    g.on_do_import(|| with(do_import));
    g.on_check_files(|| with(check_files));
}

fn id_at(c: &Ctx, row: i32) -> Option<String> {
    let i = *c.lib.shown.get(usize::try_from(row).ok()?)?;
    Some(c.model.library.get(i)?.id.clone())
}

fn item(c: &Ctx, id: &str) -> Option<MediaItem> {
    c.model.library.iter().find(|m| m.id == id).cloned()
}

/// The file is the channel's own copy (`channels/<id>/media/…`).
fn is_own(c: &Ctx, path: &str) -> bool {
    path.replace('\\', "/").starts_with(&format!("channels/{}/media/", c.model.ch()))
}

fn format_of(m: &MediaItem) -> String {
    let p = &m.probe;
    let mut parts = Vec::new();
    if let Some(v) = &p.video {
        parts.push(format!("{}×{} · {:.2} fps · {}", v.width, v.height, v.fps, v.codec));
    }
    if p.has_audio {
        parts.push(if p.audio_codec.is_empty() { "звук".into() } else { p.audio_codec.clone() });
    }
    if parts.is_empty() && m.is_url() {
        parts.push("поток (URL)".into());
    }
    parts.join(" · ")
}

fn media_row(c: &Ctx, row: usize, m: &MediaItem) -> MediaRow {
    let thumb = c.thumbs.get(&m.id);
    MediaRow {
        row: row as i32,
        id: m.id.clone().into(),
        title: if m.title.is_empty() { m.id.clone() } else { m.title.clone() }.into(),
        kind: kind_id(m.kind).into(),
        duration: hms(m.play_duration()).into(),
        category: m.category.clone().unwrap_or_default().into(),
        format: format_of(m).into(),
        problem: m.probe.error.clone().unwrap_or_default().into(),
        enabled: m.enabled,
        checked: c.lib.checked.contains(&m.id),
        has_thumb: thumb.is_some(),
        thumb: thumb.unwrap_or_default(),
    }
}

/// Recompute the visible list from filters / sorting.
pub fn refresh(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    c.lib.categories = filter::categories(&c.model.library);
    let mut cats = vec!["Все категории".to_string()];
    cats.extend(c.lib.categories.iter().cloned());
    let cat_idx = (g.get_category_index().max(0) as usize).min(cats.len() - 1);
    let f = MediaFilter {
        text: g.get_filter().to_string(),
        kinds: usize::try_from(g.get_kind_filter()).ok().and_then(|i| KINDS.get(i).copied()).into_iter().collect(),
        category: (cat_idx > 0).then(|| cats[cat_idx].clone()),
        problems_only: g.get_problems_only(),
        folder: Some(g.get_folder().to_string()),
    };
    c.lib.shown = filter::select(&c.model.library, &f, SortKey::from_index(g.get_sort_key()), g.get_sort_desc());
    let ids: HashSet<&str> = c.model.library.iter().map(|m| m.id.as_str()).collect();
    c.lib.checked.retain(|id| ids.contains(id.as_str()));
    let rows: Vec<MediaRow> = c.lib.shown.iter().enumerate().map(|(r, &i)| media_row(c, r, &c.model.library[i])).collect();
    c.lib.rows = Rc::new(VecModel::from(rows));
    g.set_items(c.lib.rows.clone().into());
    let total: i64 = c.lib.shown.iter().map(|&i| c.model.library[i].play_duration()).sum();
    let problems = c.model.library.iter().filter(|m| filter::has_problem(m)).count();
    g.set_summary(
        format!(
            "{} из {} · {}{}",
            c.lib.shown.len(),
            c.model.library.len(),
            hms(total),
            if problems > 0 { format!(" · проблемных файлов: {problems}") } else { String::new() }
        )
        .into(),
    );
    g.set_categories(vec_model(cats.into_iter().map(SharedString::from).collect()));
    g.set_category_index(cat_idx as i32);
    g.set_playlists(vec_model(
        c.model.playlists.iter().map(|p| SharedString::from(if p.name.is_empty() { p.id.clone() } else { p.name.clone() })).collect(),
    ));
    g.set_loading(!c.lib.loaded);
    folders::show(c);
    g.set_checked_count(c.lib.checked.len() as i32);
    g.set_can_browse(c.server_is_local());
    if c.prefs.library_grid != g.get_grid() {
        c.prefs.library_grid = g.get_grid();
        c.prefs.save();
    }
    lines(c);
    let wanted: Vec<String> =
        c.lib.shown.iter().take(120).map(|&i| c.model.library[i].id.clone()).filter(|id| c.thumbs.get(id).is_none()).collect();
    for id in wanted {
        thumbs::request(c, &id);
    }
}

/// Grid view: rows of `columns` tiles.
fn lines(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    let cols = c.lib.columns.max(1);
    g.set_columns(cols as i32);
    if !g.get_grid() {
        g.set_lines(vec_model(Vec::new()));
        return;
    }
    let rows: Vec<MediaRow> = c.lib.rows.iter().collect();
    let lines: Vec<TileLine> = rows.chunks(cols).map(|ch| TileLine { tiles: vec_model(ch.to_vec()) }).collect();
    g.set_lines(vec_model(lines));
}

fn update_checks(c: &mut Ctx) {
    for r in 0..c.lib.rows.row_count() {
        if let Some(mut row) = c.lib.rows.row_data(r) {
            let on = c.lib.checked.contains(row.id.as_str());
            if row.checked != on {
                row.checked = on;
                c.lib.rows.set_row_data(r, row);
            }
        }
    }
    if let Some(ui) = c.window() {
        ui.global::<Lib>().set_checked_count(c.lib.checked.len() as i32);
    }
    lines(c);
}

/// A thumbnail arrived: patch the row in place.
pub fn thumb_arrived(c: &mut Ctx, id: &str) {
    let Some(img) = c.thumbs.get(id) else { return };
    for r in 0..c.lib.rows.row_count() {
        if let Some(mut row) = c.lib.rows.row_data(r)
            && row.id == id
        {
            row.thumb = img.clone();
            row.has_thumb = true;
            c.lib.rows.set_row_data(r, row);
        }
    }
    lines(c);
}

fn open_edit(c: &mut Ctx, row: i32) {
    let Some(id) = id_at(c, row) else { return };
    let Some(m) = item(c, &id) else { return };
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    let p = &m.probe;
    let mut info = vec![format_of(&m)];
    info.push(format!("длительность {}", hms(m.duration.ms())));
    if m.in_point.is_some() || m.out_point.is_some() {
        info.push(format!(
            "обрезка {}–{}",
            m.in_point.map(|d| hms(d.ms())).unwrap_or_else(|| "начало".into()),
            m.out_point.map(|d| hms(d.ms())).unwrap_or_else(|| "конец".into())
        ));
    }
    if !p.format.is_empty() {
        info.push(format!("контейнер {}", p.format));
    }
    g.set_edit(MediaEdit {
        id: m.id.clone().into(),
        path: m.path.clone().into(),
        original: m.original_name.clone().unwrap_or_default().into(),
        own: is_own(c, &m.path),
        info: info.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n").into(),
        problem: p.error.clone().unwrap_or_default().into(),
    });
    g.set_e_title(m.title.clone().into());
    g.set_e_kind(KINDS.iter().position(|k| *k == m.kind).unwrap_or(0) as i32);
    g.set_e_category(m.category.clone().unwrap_or_default().into());
    g.set_e_volume(format!("{}", m.volume_db).into());
    g.set_e_weight(format!("{}", m.weight).into());
    g.set_e_enabled(m.enabled);
    g.set_e_folder(folders::option_index(c, &m.folder));
    g.set_e_volume_error(SharedString::new());
    g.set_e_weight_error(SharedString::new());
    g.set_focused(row);
    g.set_edit_open(true);
    c.lib.edit_id = Some(id);
}

fn parse_num(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").replace('−', "-").parse().ok()
}

fn validate_edit(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    let vol = match parse_num(&g.get_e_volume()) {
        Some(v) if (-30.0..=20.0).contains(&v) => "",
        _ => "Число от −30 до 20",
    };
    let w = match parse_num(&g.get_e_weight()) {
        Some(v) if (0.0..=100.0).contains(&v) => "",
        _ => "Число от 0 до 100",
    };
    g.set_e_volume_error(vol.into());
    g.set_e_weight_error(w.into());
}

fn save_edit(c: &mut Ctx) {
    validate_edit(c);
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    if !g.get_e_volume_error().is_empty() || !g.get_e_weight_error().is_empty() {
        return;
    }
    let Some(mut m) = c.lib.edit_id.clone().and_then(|id| item(c, &id)) else { return };
    m.title = g.get_e_title().trim().to_string();
    m.kind = KINDS.get(g.get_e_kind().max(0) as usize).copied().unwrap_or(m.kind);
    let cat = g.get_e_category().trim().to_string();
    m.category = (!cat.is_empty()).then_some(cat);
    m.volume_db = parse_num(&g.get_e_volume()).unwrap_or(0.0) as f32;
    m.weight = parse_num(&g.get_e_weight()).unwrap_or(1.0);
    m.enabled = g.get_e_enabled();
    m.folder = folders::option_path(c, g.get_e_folder());
    g.set_edit_open(false);
    put_media(c, m, "Сохранено");
}

fn put_media(c: &mut Ctx, m: MediaItem, done: &'static str) {
    let ch = c.model.ch();
    c.change(move |api| {
        api.put(&format!("/channels/{ch}/media/{}", m.id), &m)?;
        Ok(done.into())
    });
}

fn row_action(c: &mut Ctx, row: i32, action: &str) {
    let Some(id) = id_at(c, row) else { return };
    let title = item(c, &id).map(|m| m.title).unwrap_or_else(|| id.clone());
    match action {
        "edit" => open_edit(c, row),
        "queue" => c.command(json!({ "cmd": "queue", "media": id })),
        "play-now" => c.ask("Показать сейчас?", &format!("Текущий элемент будет прерван, в эфир пойдёт «{title}»."), "Показать сейчас", move |c| {
            c.command(json!({ "cmd": "play_now", "media": id }))
        }),
        "add-to-open-playlist" => {
            if c.pl.is_open() {
                super::playlists::append(c, &id);
                c.toast(format!("«{title}» добавлен в открытый плейлист (сохраните его в разделе «Плейлисты»)"), false);
            } else {
                c.toast("Сначала откройте плейлист в разделе «Плейлисты»", true);
            }
        }
        "toggle" => {
            if let Some(mut m) = item(c, &id) {
                m.enabled = !m.enabled;
                put_media(c, m, "Сохранено");
            }
        }
        "delete" => c.ask(
            "Удалить из медиатеки?",
            &format!(
                "«{title}» будет удалён из медиатеки. {}",
                if item(c, &id).is_some_and(|m| is_own(c, &m.path)) {
                    "Его копия в хранилище канала тоже будет удалена."
                } else {
                    "Файл на диске останется."
                }
            ),
            "Удалить",
            move |c| delete_ids(c, vec![id]),
        ),
        _ => {}
    }
}

fn delete_ids(c: &mut Ctx, ids: Vec<String>) {
    let ch = c.model.ch();
    let n = ids.len();
    c.change(move |api| {
        for id in &ids {
            api.delete(&format!("/channels/{ch}/media/{id}"))?;
        }
        Ok(format!("Удалено: {n}"))
    });
    c.lib.checked.clear();
}

fn bulk(c: &mut Ctx, action: &str, arg: i32) {
    let ids: Vec<String> =
        c.lib.shown.iter().map(|&i| c.model.library[i].id.clone()).filter(|id| c.lib.checked.contains(id)).collect();
    if ids.is_empty() {
        return;
    }
    let ch = c.model.ch();
    match action {
        "playlist" => {
            let Some(mut p) = c.model.playlists.get(arg as usize).cloned() else { return };
            let n = ids.len();
            p.entries.extend(ids.into_iter().map(PlaylistEntry::new));
            let name = if p.name.is_empty() { p.id.clone() } else { p.name.clone() };
            c.change(move |api| {
                api.put(&format!("/channels/{ch}/playlists/{}", p.id), &p)?;
                Ok(format!("Добавлено в «{name}»: {n}"))
            });
            c.lib.checked.clear();
        }
        "queue" => {
            c.worker.spawn(move |api| {
                for id in &ids {
                    api.post(&format!("/channels/{ch}/command"), &json!({ "cmd": "queue", "media": id }))?;
                }
                Ok(Msg::Done(format!("В очереди: {}", ids.len())))
            });
        }
        "enable" | "disable" => {
            let on = action == "enable";
            let items: Vec<MediaItem> = ids.iter().filter_map(|id| item(c, id)).map(|mut m| {
                m.enabled = on;
                m
            }).collect();
            c.change(move |api| {
                for m in &items {
                    api.put(&format!("/channels/{ch}/media/{}", m.id), m)?;
                }
                Ok(format!("{}: {}", if on { "Включено" } else { "Выключено" }, items.len()))
            });
        }
        "folder" => {
            let path = folders::option_path(c, arg);
            let n = ids.len();
            let body = json!({ "op": "move", "path": path, "ids": ids });
            let label = if path.is_empty() { "верхний уровень".to_string() } else { format!("«{path}»") };
            c.change(move |api| {
                api.post(&format!("/channels/{ch}/media/folders"), &body)?;
                Ok(format!("Перемещено в {label}: {n}"))
            });
            c.lib.checked.clear();
        }
        "kind" => {
            let Some(k) = KINDS.get(arg as usize).copied() else { return };
            let items: Vec<MediaItem> = ids.iter().filter_map(|id| item(c, id)).map(|mut m| {
                m.kind = k;
                m
            }).collect();
            c.change(move |api| {
                for m in &items {
                    api.put(&format!("/channels/{ch}/media/{}", m.id), m)?;
                }
                Ok(format!("Тип изменён: {}", items.len()))
            });
        }
        "delete" => {
            let n = ids.len();
            c.ask(
                "Удалить выбранные ролики?",
                &format!("Из медиатеки будет удалено элементов: {n}. Копии в хранилище канала удаляются, остальные файлы на диске останутся."),
                "Удалить",
                move |c| delete_ids(c, ids),
            );
        }
        _ => {}
    }
}

/// Extensions offered by the file picker (the server probes the rest).
const MEDIA_EXT: &[&str] =
    &["mp4", "mov", "mkv", "avi", "ts", "mts", "m2ts", "mxf", "webm", "flv", "wmv", "mpg", "mpeg", "m4v", "mp3", "wav", "aac", "flac", "m4a", "ogg"];

/// Several picked files share the path field.
const PATH_SEP: &str = "; ";

fn import_paths(field: &str) -> Vec<String> {
    field.split(';').map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect()
}

fn do_import(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    let paths = import_paths(&g.get_import_path());
    if paths.is_empty() {
        return;
    }
    let kind: Option<MediaKind> = usize::try_from(g.get_import_kind() - 1).ok().and_then(|i| KINDS.get(i).copied());
    let cat = g.get_import_category().trim().to_string();
    let copy = g.get_import_copy();
    let body = json!({
        "paths": paths,
        "kind": kind,
        "category": if cat.is_empty() { Value::Null } else { json!(cat) },
        "recursive": g.get_import_recursive(),
        "folder": folders::option_path(c, g.get_import_folder()),
        "copy": copy,
    });
    g.set_importing(true);
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let r = api.post(&format!("/channels/{ch}/media/import"), &body);
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if let Some(ui) = c.window() {
                ui.global::<Lib>().set_importing(false);
            }
            match r {
                Ok(r) if r["job"].is_u64() => {
                    super::jobs::started(c, r["job"].as_u64().unwrap_or_default());
                    if let Some(ui) = c.window() {
                        let g = ui.global::<Lib>();
                        g.set_import_open(false);
                        g.set_import_path(SharedString::new());
                    }
                    c.toast("Копирование началось — прогресс в шапке окна", false);
                }
                Ok(r) => {
                    let added = r["added"].as_array().map(|a| a.len()).unwrap_or(0);
                    let errors: Vec<String> =
                        r["errors"].as_array().map(|a| a.iter().filter_map(|e| e.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    if added == 0 && !errors.is_empty() {
                        c.toast(format!("Импорт не выполнен: {}", errors.join("; ")), true);
                        return;
                    }
                    if let Some(ui) = c.window() {
                        let g = ui.global::<Lib>();
                        g.set_import_open(false);
                        g.set_import_path(SharedString::new());
                    }
                    c.toast(
                        format!(
                            "Добавлено: {added}{}",
                            if errors.is_empty() { String::new() } else { format!(", ошибок: {}", errors.len()) }
                        ),
                        !errors.is_empty(),
                    );
                    c.load_all();
                }
                Err(e) => c.toast(format!("Импорт: {e}"), true),
            }
        })))
    });
}

fn check_files(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    ui.global::<Lib>().set_checking(true);
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let r = api.post(&format!("/channels/{ch}/media/check"), &json!({}));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if let Some(ui) = c.window() {
                ui.global::<Lib>().set_checking(false);
            }
            match r {
                Ok(r) => {
                    let n = r["problems"].as_array().map(|a| a.len()).unwrap_or(0);
                    if n == 0 {
                        c.toast("Все файлы в порядке", false);
                    } else {
                        c.toast(format!("Проблемных файлов: {n} — включён фильтр «Проблемные»"), true);
                        if let Some(ui) = c.window() {
                            ui.global::<Lib>().set_problems_only(true);
                        }
                    }
                    c.load_all();
                }
                Err(e) => c.toast(format!("Проверка файлов: {e}"), true),
            }
        })))
    });
}

pub fn apply_prefs(c: &mut Ctx) {
    if let Some(ui) = c.window() {
        ui.global::<Lib>().set_grid(c.prefs.library_grid);
    }
}

pub fn shortcut(c: &mut Ctx, name: &str) {
    if name == "delete" && !c.lib.checked.is_empty() {
        bulk(c, "delete", 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_files_in_one_field() {
        assert_eq!(import_paths("D:/a.mp4; D:/b c.mov;;"), ["D:/a.mp4", "D:/b c.mov"]);
        assert_eq!(import_paths("  https://x/y  "), ["https://x/y"]);
        assert!(import_paths(" ; ").is_empty());
    }
}

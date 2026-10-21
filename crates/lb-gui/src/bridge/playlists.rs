//! Playlists screen: list, draft editing with undo, drag & drop, entry
//! actions, add from library, create / delete / export.

use slint::{ComponentHandle, SharedString};

use lb_core::media::{Library, MediaKind};
use lb_core::playlist::{Playlist, PlaylistEntry};

use super::{Ctx, thumbs, vec_model, with};
use crate::fmt::{KINDS, hms, kind_id};
use crate::logic::filter::has_problem;
use crate::logic::undo::History;
use crate::net::Msg;
use crate::shell::{AppWindow, EntryProps, EntryRow, MediaPick, Pl, PlaylistItem};

#[derive(Default)]
pub struct PlaylistsState {
    selected: Option<String>,
    /// Last version received from the server.
    saved: Option<Playlist>,
    draft: Option<Playlist>,
    history: History<Playlist>,
    /// Playlist ids in the order shown in the list (after filtering).
    shown: Vec<String>,
    entry: Option<usize>,
}

impl PlaylistsState {
    /// A playlist is open in the editor.
    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    pub fn dirty(&self) -> bool {
        self.draft.is_some() && self.draft != self.saved
    }
}

pub fn connect(ui: &AppWindow) {
    let p = ui.global::<Pl>();
    p.on_open(|i| with(move |c| open_index(c, i as usize)));
    p.on_filter_lists(|_| with(lists_changed));
    p.on_edited(|| with(read_header));
    p.on_move(|from, to| with(move |c| move_entry(c, from as usize, to as usize)));
    p.on_entry_action(|i, action| with(move |c| entry_action(c, i as usize, action.as_str())));
    p.on_add_media(|id| with(move |c| add_media(c, id.to_string())));
    p.on_search_add(|_| with(add_results));
    p.on_filter_entries(|| with(rows_changed));
    p.on_save(|| with(save));
    p.on_revert(|| {
        with(|c| {
            let saved = c.pl.saved.clone();
            mutate(c, move |p| {
                if let Some(s) = saved {
                    *p = s;
                }
            })
        })
    });
    p.on_undo(|| with(undo));
    p.on_redo(|| with(redo));
    p.on_validate_new_id(|id| with(move |c| validate_new_id(c, id.as_str())));
    p.on_create(|| with(create));
    p.on_menu_action(|a| with(move |c| menu_action(c, a.as_str())));
}

/// Fresh data from the server.
pub fn reloaded(c: &mut Ctx) {
    let server = c.pl.selected.as_ref().and_then(|id| c.model.playlists.iter().find(|p| &p.id == id).cloned());
    let was_dirty = c.pl.dirty();
    c.pl.saved = server.clone();
    if !was_dirty || c.pl.draft.is_none() {
        c.pl.draft = server;
    }
    if c.pl.selected.is_some() && c.pl.saved.is_none() && !was_dirty {
        // deleted on the server
        c.pl.selected = None;
        c.pl.draft = None;
        c.pl.history.clear();
    }
    lists_changed(c);
    rows_changed(c);
}

pub fn lists_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Pl>();
    let lib = Library::new(c.model.library.clone());
    let f = g.get_list_filter().to_lowercase();
    let mut items = Vec::new();
    c.pl.shown.clear();
    for p in &c.model.playlists {
        if !f.is_empty() && !p.name.to_lowercase().contains(&f) && !p.id.to_lowercase().contains(&f) {
            continue;
        }
        let problems = p.entries.iter().filter(|e| lib.get(&e.media).is_none_or(has_problem)).count();
        items.push(PlaylistItem {
            id: p.id.clone().into(),
            name: if p.name.is_empty() { p.id.clone() } else { p.name.clone() }.into(),
            meta: format!("{} · {}{}", p.entries.len(), hms(p.total_duration(&lib)), if p.shuffle { " · случайно" } else { "" }).into(),
            problems: problems as i32,
        });
        c.pl.shown.push(p.id.clone());
    }
    let current = c.pl.selected.as_ref().and_then(|s| c.pl.shown.iter().position(|x| x == s)).map(|i| i as i32).unwrap_or(-1);
    g.set_lists(vec_model(items));
    g.set_current(current);
}

fn open_index(c: &mut Ctx, i: usize) {
    let Some(id) = c.pl.shown.get(i).cloned() else { return };
    if c.pl.selected.as_deref() == Some(id.as_str()) {
        return;
    }
    if c.pl.dirty() {
        c.ask("Несохранённые изменения", "Изменения в текущем плейлисте будут потеряны.", "Не сохранять", move |c| open_id(c, id));
    } else {
        open_id(c, id);
    }
}

pub fn open_id(c: &mut Ctx, id: String) {
    c.pl.saved = c.model.playlists.iter().find(|p| p.id == id).cloned();
    c.pl.draft = c.pl.saved.clone();
    c.pl.selected = Some(id);
    c.pl.history.clear();
    c.pl.entry = None;
    if let Some(ui) = c.window() {
        let g = ui.global::<Pl>();
        g.set_entry_filter(SharedString::new());
        g.set_kind_filter(-1);
    }
    lists_changed(c);
    rows_changed(c);
}

/// Apply an edit to the draft with undo support.
fn mutate(c: &mut Ctx, f: impl FnOnce(&mut Playlist)) {
    let Some(before) = c.pl.draft.clone() else { return };
    let mut after = before.clone();
    f(&mut after);
    c.pl.history.record(&before, &after);
    c.pl.draft = Some(after);
    rows_changed(c);
}

/// Name / loop / shuffle edited in the header.
fn read_header(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Pl>();
    let (name, looped, shuffle) = (g.get_name().to_string(), g.get_looped(), g.get_shuffle());
    mutate(c, move |p| {
        p.name = name;
        p.looped = looped;
        p.shuffle = shuffle;
    });
}

fn move_entry(c: &mut Ctx, from: usize, to: usize) {
    mutate(c, |p| {
        let to = to.min(p.entries.len().saturating_sub(1));
        p.move_entry(from, to);
    });
    c.pl.entry = Some(to);
    rows_changed(c);
}

fn entry_action(c: &mut Ctx, i: usize, action: &str) {
    let len = c.pl.draft.as_ref().map(|p| p.entries.len()).unwrap_or(0);
    if i >= len {
        return;
    }
    match action {
        "select" => {
            c.pl.entry = Some(i);
            rows_changed(c);
        }
        "up" if i > 0 => move_entry(c, i, i - 1),
        "down" if i + 1 < len => move_entry(c, i, i + 1),
        "top" => move_entry(c, i, 0),
        "bottom" => move_entry(c, i, len - 1),
        "duplicate" => {
            mutate(c, |p| {
                let e = p.entries[i].clone();
                p.entries.insert(i + 1, e);
            });
        }
        "pin" => mutate(c, |p| p.entries[i].pinned = !p.entries[i].pinned),
        "disable" => mutate(c, |p| p.entries[i].disabled = !p.entries[i].disabled),
        "queue" => {
            let media = c.pl.draft.as_ref().map(|p| p.entries[i].media.clone()).unwrap_or_default();
            c.command(serde_json::json!({ "cmd": "queue", "media": media }));
        }
        "remove" => {
            let title = entry_title(c, i);
            mutate(c, |p| {
                p.entries.remove(i);
            });
            c.pl.entry = None;
            rows_changed(c);
            c.toast_with(format!("Удалено: {title}"), "Отменить", undo);
        }
        _ => {}
    }
}

fn entry_title(c: &Ctx, i: usize) -> String {
    let Some(e) = c.pl.draft.as_ref().and_then(|p| p.entries.get(i)) else { return String::new() };
    c.model.library.iter().find(|m| m.id == e.media).map(|m| m.title.clone()).unwrap_or_else(|| e.media.clone())
}

/// Append a media item to the open draft (from the library screen).
pub fn append(c: &mut Ctx, id: &str) {
    let id = id.to_string();
    mutate(c, move |p| p.entries.push(PlaylistEntry::new(id)));
}

fn add_media(c: &mut Ctx, id: String) {
    if c.pl.draft.is_none() {
        return;
    }
    let title = c.model.library.iter().find(|m| m.id == id).map(|m| m.title.clone()).unwrap_or_else(|| id.clone());
    mutate(c, move |p| p.entries.push(PlaylistEntry::new(id)));
    c.toast_with(format!("Добавлено: {title}"), "Отменить", undo);
}

fn undo(c: &mut Ctx) {
    let Some(cur) = c.pl.draft.clone() else { return };
    if let Some(prev) = c.pl.history.undo(&cur) {
        c.pl.draft = Some(prev);
        rows_changed(c);
    }
}

fn redo(c: &mut Ctx) {
    let Some(cur) = c.pl.draft.clone() else { return };
    if let Some(next) = c.pl.history.redo(&cur) {
        c.pl.draft = Some(next);
        rows_changed(c);
    }
}

fn save(c: &mut Ctx) {
    let Some(p) = c.pl.draft.clone() else { return };
    if !c.pl.dirty() {
        return;
    }
    let path = format!("/playlists/{}", p.id);
    c.save_doc(&path, p, "плейлист");
}

/// Keyboard shortcuts while the screen is open.
pub fn shortcut(c: &mut Ctx, name: &str) {
    match name {
        "save" => save(c),
        "undo" => undo(c),
        "redo" => redo(c),
        "delete" => {
            if let Some(i) = c.pl.entry {
                entry_action(c, i, "remove");
            }
        }
        _ => {}
    }
}

fn kind_filter(i: i32) -> Option<MediaKind> {
    usize::try_from(i).ok().and_then(|i| KINDS.get(i).copied())
}

/// Push the draft into the editor (rows, header, summary, flags).
pub fn rows_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Pl>();
    let Some(p) = c.pl.draft.clone() else {
        g.set_has_draft(false);
        g.set_entries(vec_model(Vec::new()));
        g.set_props(EntryProps::default());
        return;
    };
    let lib = Library::new(c.model.library.clone());
    let text = g.get_entry_filter().to_lowercase();
    let kind = kind_filter(g.get_kind_filter());
    let filtered = !text.trim().is_empty() || kind.is_some();
    let mut rows = Vec::new();
    let mut offset = 0;
    let mut missing = 0;
    let mut wanted = Vec::new();
    for (i, e) in p.entries.iter().enumerate() {
        let m = lib.get(&e.media);
        let d = m.map(|m| m.play_duration()).unwrap_or(0);
        let start = offset;
        if !e.disabled {
            offset += d;
        }
        if m.is_none_or(has_problem) {
            missing += 1;
        }
        let title = m.map(|m| m.title.clone()).unwrap_or_default();
        if let Some(k) = kind
            && m.map(|m| m.kind) != Some(k)
        {
            continue;
        }
        if !text.trim().is_empty() && !title.to_lowercase().contains(text.trim()) && !e.media.to_lowercase().contains(text.trim()) {
            continue;
        }
        let thumb = c.thumbs.get(&e.media);
        if thumb.is_none() && m.is_some() {
            wanted.push(e.media.clone());
        }
        rows.push(EntryRow {
            index: i as i32,
            media: e.media.clone().into(),
            title: title.into(),
            kind: m.map(|m| kind_id(m.kind)).unwrap_or("filler").into(),
            duration: hms(d).into(),
            offset: format!("+{}", hms(start)).into(),
            pinned: e.pinned,
            disabled: e.disabled,
            missing: m.is_none(),
            problem: m.and_then(|m| m.probe.error.clone()).map(|e| format!("Файл: {e}")).unwrap_or_default().into(),
            has_thumb: thumb.is_some(),
            thumb: thumb.unwrap_or_default(),
        });
    }
    let enabled = p.entries.iter().filter(|e| !e.disabled).count();
    g.set_has_draft(true);
    g.set_name(p.name.clone().into());
    g.set_looped(p.looped);
    g.set_shuffle(p.shuffle);
    g.set_entries(vec_model(rows));
    g.set_summary(
        format!(
            "{} элементов ({} включено) · {}{}",
            p.entries.len(),
            enabled,
            hms(p.total_duration(&lib)),
            if p.looped { " · по кругу" } else { "" }
        )
        .into(),
    );
    g.set_missing_count(missing);
    g.set_dirty(c.pl.dirty());
    g.set_can_undo(c.pl.history.can_undo());
    g.set_can_redo(c.pl.history.can_redo());
    g.set_filtered(filtered);
    let sel = c.pl.entry.filter(|&i| i < p.entries.len());
    g.set_selected(sel.map(|i| i as i32).unwrap_or(-1));
    g.set_props(match sel {
        Some(i) => {
            let e = &p.entries[i];
            let m = lib.get(&e.media);
            EntryProps {
                has: true,
                index: i as i32,
                media: e.media.clone().into(),
                title: m.map(|m| m.title.clone()).unwrap_or_else(|| format!("Нет в медиатеке: {}", e.media)).into(),
                kind: m.map(|m| kind_id(m.kind)).unwrap_or("filler").into(),
                category: m.and_then(|m| m.category.clone()).unwrap_or_default().into(),
                duration: hms(m.map(|m| m.play_duration()).unwrap_or(0)).into(),
                path: m.map(|m| m.path.clone()).unwrap_or_default().into(),
                pinned: e.pinned,
                disabled: e.disabled,
                problem: if m.is_none() {
                    "Ролика нет в медиатеке".into()
                } else {
                    m.and_then(|m| m.probe.error.clone()).unwrap_or_default().into()
                },
            }
        }
        None => EntryProps::default(),
    });
    for id in wanted.into_iter().take(64) {
        thumbs::request(c, &id);
    }
}

pub fn add_results(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Pl>();
    let f = g.get_add_filter().to_lowercase();
    let rows: Vec<MediaPick> = c
        .model
        .library
        .iter()
        .filter(|m| m.enabled)
        .filter(|m| {
            f.is_empty()
                || m.title.to_lowercase().contains(&f)
                || m.id.to_lowercase().contains(&f)
                || m.category.as_deref().is_some_and(|x| x.to_lowercase().contains(&f))
        })
        .take(200)
        .map(|m| MediaPick {
            id: m.id.clone().into(),
            title: m.title.clone().into(),
            kind: kind_id(m.kind).into(),
            duration: hms(m.play_duration()).into(),
        })
        .collect();
    g.set_add_results(vec_model(rows));
}

/// Inline validation of the new playlist id.
fn validate_new_id(c: &mut Ctx, id: &str) {
    let Some(ui) = c.window() else { return };
    let id = id.trim();
    let err = if id.is_empty() {
        ""
    } else if !id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        "Допустимы латиница, цифры, «-» и «_»"
    } else if c.model.playlists.iter().any(|p| p.id == id) {
        "Плейлист с таким идентификатором уже есть"
    } else {
        ""
    };
    ui.global::<Pl>().set_new_id_error(err.into());
}

fn create(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Pl>();
    let id = g.get_new_id().trim().to_string();
    validate_new_id(c, &id);
    if id.is_empty() || !g.get_new_id_error().is_empty() {
        return;
    }
    let mut p = Playlist::new(id.clone());
    p.name = g.get_new_name().trim().to_string();
    g.set_create_open(false);
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        api.post(&format!("/channels/{ch}/playlists"), &p)?;
        Ok(Msg::Done("Плейлист создан".into()))
    });
    // open it once the reload arrives
    c.pl.selected = Some(id);
    c.pl.draft = None;
    c.pl.saved = None;
    c.pl.history.clear();
    c.load_all();
}

fn menu_action(c: &mut Ctx, action: &str) {
    let Some(id) = c.pl.selected.clone() else { return };
    match action {
        "clear" => {
            mutate(c, |p| p.entries.clear());
            c.toast_with("Плейлист очищен (не сохранено)", "Отменить", undo);
        }
        "delete" => {
            let name = c.pl.draft.as_ref().map(|p| if p.name.is_empty() { p.id.clone() } else { p.name.clone() }).unwrap_or_default();
            c.ask(
                "Удалить плейлист?",
                &format!("Плейлист «{name}» будет удалён с сервера. Блоки сетки, которые на него ссылаются, перестанут работать."),
                "Удалить",
                move |c| {
                    let ch = c.model.ch();
                    let id2 = id.clone();
                    c.worker.spawn(move |api| {
                        api.delete(&format!("/channels/{ch}/playlists/{id2}"))?;
                        Ok(Msg::Done("Плейлист удалён".into()))
                    });
                    c.pl = PlaylistsState::default();
                    rows_changed(c);
                    c.load_all();
                },
            );
        }
        "export" => {
            let Some(path) = rfd::FileDialog::new().set_file_name(format!("{id}.m3u")).add_filter("M3U", &["m3u", "m3u8"]).save_file() else {
                return;
            };
            let ch = c.model.ch();
            c.worker.spawn(move |api| {
                let bytes = api.get_bytes(&format!("/channels/{ch}/playlists/{id}/m3u"))?;
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                Ok(Msg::Done(format!("Сохранено: {}", path.display())))
            });
        }
        _ => {}
    }
}

pub fn apply_prefs(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    if let Some(w) = c.prefs.panels.get("playlists.list") {
        ui.global::<Pl>().set_list_width(*w);
    }
}

//! Scenes screen: library items composed on air (background + music by
//! rules) with their own graphics rules.
//!
//! * [`form`] — general settings and background of the draft
//! * [`music`] — music rules of the draft
//! * [`gfx`] — the scene's own graphics, channel graphics over it
//! * [`templates`] — new scenes (broadcast clock, music break, up next)

mod form;
mod gfx;
pub(crate) mod music;
mod templates;

use serde_json::json;
use slint::ComponentHandle;

use lb_core::media::MediaItem;

use super::picker::{self, Target};
use super::{Ctx, vec_model, with};
use crate::fmt::hms;
use crate::shell::{AppWindow, SceneItemRow, Scenes};

/// Library folder new scenes go to.
pub const FOLDER: &str = "Сцены";

#[derive(Default)]
pub struct ScenesState {
    /// Id of the selected scene.
    pub(crate) selected: Option<String>,
    /// Edited copy of the selected scene.
    draft: Option<MediaItem>,
    dirty: bool,
    /// A scene just created: select it when the library arrives.
    pending: Option<String>,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Scenes>();
    g.on_select(|i| with(move |c| select(c, i)));
    g.on_tab_changed(|i| {
        with(move |c| {
            if i > 0 {
                super::rules::opened(c, "scenes");
            }
        })
    });
    g.on_create(|t| with(move |c| templates::create(c, t.as_str())));
    g.on_duplicate(|| with(duplicate));
    g.on_remove(|| with(remove));
    g.on_form_changed(|| with(form::changed));
    g.on_bg_pick(|| with(|c| picker::open(c, Target::SceneBackground, "Фон сцены: картинка или видео")));
    g.on_save(|| with(save));
    g.on_revert(|| {
        with(|c| {
            c.scenes.dirty = false;
            c.scenes.draft = None;
            refresh(c);
        })
    });
    g.on_air(|a| {
        with(move |c| {
            let Some(id) = c.scenes.selected.clone() else { return };
            let cmd = if a == "play" { "play_now" } else { "queue" };
            c.command(json!({ "cmd": cmd, "media": id }));
        })
    });
    music::connect(&g);
    gfx::connect(&g);
}

/// Scenes of the library, in title order.
fn scenes(c: &Ctx) -> Vec<&MediaItem> {
    let mut v: Vec<&MediaItem> = c.model.library.iter().filter(|m| m.is_scene()).collect();
    v.sort_by_key(|m| m.title.to_lowercase());
    v
}

fn title(m: &MediaItem) -> String {
    if m.title.is_empty() { m.id.clone() } else { m.title.clone() }
}

/// Library reloaded (or the screen opened): list and form.
pub fn refresh(c: &mut Ctx) {
    if let Some(id) = c.scenes.pending.take() {
        if c.model.library.iter().any(|m| m.id == id) {
            c.scenes.selected = Some(id);
            c.scenes.draft = None;
            c.scenes.dirty = false;
        } else {
            c.scenes.pending = Some(id);
        }
    }
    let list: Vec<MediaItem> = scenes(c).into_iter().cloned().collect();
    if c.scenes.selected.as_ref().is_some_and(|id| !list.iter().any(|m| &m.id == id)) && !c.scenes.dirty {
        c.scenes.selected = None;
        c.scenes.draft = None;
    }
    if !c.scenes.dirty {
        c.scenes.draft = c.scenes.selected.as_ref().and_then(|id| list.iter().find(|m| &m.id == id)).cloned();
    }
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Scenes>();
    let rows: Vec<SceneItemRow> = list.iter().map(|m| row(c, m)).collect();
    g.set_rows(vec_model(rows));
    g.set_selected(c.scenes.selected.as_ref().and_then(|id| list.iter().position(|m| &m.id == id)).map_or(-1, |i| i as i32));
    g.set_dirty(c.scenes.dirty);
    let folders: Vec<String> = std::iter::once("нет".to_string()).chain(lb_core::media::Library::new(c.model.library.clone()).all_folders()).collect();
    g.set_folder_options(vec_model(folders.into_iter().map(Into::into).collect()));
    form::show(c);
    music::show(c);
    gfx::show(c);
}

fn row(c: &Ctx, m: &MediaItem) -> SceneItemRow {
    use lb_core::media::scene::Background;
    let Some(s) = &m.scene else { return SceneItemRow::default() };
    let bg = match &s.background {
        Background::Color { .. } => "фон: цвет".to_string(),
        Background::Media { media } => format!("фон: {}", c.model.library.iter().find(|x| &x.id == media).map(title).unwrap_or_else(|| media.clone())),
    };
    let music = match s.music.iter().filter(|r| r.enabled).count() {
        0 => "без музыки".to_string(),
        1 => "музыка".to_string(),
        n => format!("музыка: {n} правила"),
    };
    let problem = match &s.background {
        Background::Media { media } if !c.model.library.iter().any(|x| &x.id == media) => format!("фон «{media}» не найден в медиатеке"),
        _ => String::new(),
    };
    SceneItemRow { id: m.id.clone().into(), title: title(m).into(), detail: format!("{} · {bg} · {music}", hms(m.play_duration())).into(), problem: problem.into() }
}

pub fn select(c: &mut Ctx, i: i32) {
    if c.scenes.dirty {
        let i2 = i;
        c.ask("Сохранить изменения сцены?", "Изменения выбранной сцены ещё не сохранены.", "Не сохранять", move |c| {
            c.scenes.dirty = false;
            select(c, i2);
        });
        return;
    }
    let list: Vec<String> = scenes(c).iter().map(|m| m.id.clone()).collect();
    c.scenes.selected = usize::try_from(i).ok().and_then(|i| list.get(i).cloned());
    c.scenes.draft = None;
    refresh(c);
}

/// The draft changed: mark unsaved.
fn touched(c: &mut Ctx) {
    c.scenes.dirty = true;
    if let Some(ui) = c.window() {
        ui.global::<Scenes>().set_dirty(true);
    }
}

fn save(c: &mut Ctx) {
    let Some(mut item) = c.scenes.draft.clone() else { return };
    // the graphics are edited on the graphics screen: keep their latest copy
    let latest = c.model.library.iter().find(|m| m.id == item.id).and_then(|m| m.scene.as_ref()).map(|s| s.graphics.clone());
    if let (Some(s), Some(g)) = (item.scene.as_mut(), latest) {
        s.graphics = g;
    }
    if let Some(ui) = c.window()
        && !ui.global::<Scenes>().get_e_duration().is_empty()
    {
        c.toast("Исправьте длительность сцены: ЧЧ:ММ:СС", true);
        return;
    }
    c.scenes.dirty = false;
    let ch = c.model.ch();
    let name = title(&item);
    c.change(move |api| {
        api.put(&format!("/channels/{ch}/media/{}", item.id), &item)?;
        Ok(format!("Сцена «{name}» сохранена"))
    });
}

/// Ctrl+S on this screen.
pub fn shortcut(c: &mut Ctx, name: &str) {
    // the rules tabs edit the rules draft
    if c.window().is_some_and(|ui| ui.global::<Scenes>().get_tab() > 0) {
        return super::rules::shortcut(c, name);
    }
    if name == "save" && c.scenes.dirty {
        save(c);
    }
}

/// Add a new library item and select it once it is there.
fn add_item(c: &mut Ctx, item: MediaItem, done: String) {
    c.scenes.pending = Some(item.id.clone());
    let ch = c.model.ch();
    c.change(move |api| {
        api.post(&format!("/channels/{ch}/media"), &item)?;
        Ok(done)
    });
}

fn duplicate(c: &mut Ctx) {
    let Some(mut item) = c.scenes.draft.clone() else { return };
    let lib = lb_core::media::Library::new(c.model.library.clone());
    item.id = lib.unique_id(&item.id);
    item.title = format!("{} (копия)", title(&item));
    let done = format!("Сцена «{}» создана", item.title);
    add_item(c, item, done);
}

fn remove(c: &mut Ctx) {
    let Some(item) = c.scenes.draft.clone() else { return };
    let name = title(&item);
    c.ask(&format!("Удалить сцену «{name}»?"), "Сцена будет удалена из медиатеки вместе со своей графикой.", "Удалить", move |c| {
        c.scenes.dirty = false;
        c.scenes.selected = None;
        c.scenes.draft = None;
        let ch = c.model.ch();
        let id = item.id.clone();
        c.change(move |api| {
            api.delete(&format!("/channels/{ch}/media/{id}"))?;
            Ok(format!("Сцена «{name}» удалена"))
        });
    });
}

/// Ids chosen in the library picker for this screen.
pub fn picked(c: &mut Ctx, target: Target, ids: Vec<String>) {
    match target {
        Target::SceneBackground => form::background_picked(c, ids),
        Target::SceneTracks(i) => music::tracks_picked(c, i, ids),
        _ => {}
    }
}

/// A thumbnail came: the background preview may be it.
pub fn thumb_arrived(c: &mut Ctx) {
    form::show_thumb(c);
}

/// Graphics reloaded: the scene's graphics rules.
pub fn graphics_changed(c: &mut Ctx) {
    gfx::show(c);
}

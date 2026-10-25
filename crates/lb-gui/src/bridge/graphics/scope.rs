//! What the editor edits: the channel graphics (the whole broadcast) or
//! the own graphics of one scene. The two are kept apart — saving one
//! never writes the other — so a scene made from a template cannot reset
//! the channel graphics, and the channel graphics never leak into a scene
//! document.

use slint::ComponentHandle;

use lb_core::graphics::GraphicsConfig;
use lb_core::media::MediaItem;

use super::{Ctx, vec_model, with};
use crate::shell::Gfx;

pub fn connect(g: &Gfx) {
    g.on_scope_changed(|i| with(move |c| pick(c, i)));
}

/// Scenes of the library in title order: (id, title).
fn scenes(c: &Ctx) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> =
        c.model.library.iter().filter(|m| m.is_scene()).map(|m| (m.id.clone(), if m.title.is_empty() { m.id.clone() } else { m.title.clone() })).collect();
    v.sort_by_key(|(_, t)| t.to_lowercase());
    v
}

/// The scene being edited (`None`: the channel graphics).
pub fn scene(c: &Ctx) -> Option<&MediaItem> {
    let id = c.gfx.scope.as_deref()?;
    c.model.library.iter().find(|m| m.id == id && m.is_scene())
}

/// The saved document of the current scope.
pub fn source(c: &Ctx) -> GraphicsConfig {
    match &c.gfx.scope {
        None => c.model.graphics.clone(),
        Some(_) => scene(c).and_then(|m| m.scene.as_ref()).map(|s| s.graphics.clone()).unwrap_or_default(),
    }
}

/// Fill the scope select.
pub fn show(c: &mut Ctx) {
    // a removed scene: back to the channel graphics
    if c.gfx.scope.is_some() && scene(c).is_none() && c.lib.loaded {
        c.gfx.scope = None;
    }
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let list = scenes(c);
    let names: Vec<slint::SharedString> = std::iter::once("Весь эфир".to_string()).chain(list.iter().map(|(_, t)| format!("Сцена: {t}"))).map(Into::into).collect();
    g.set_scopes(vec_model(names));
    let i = c.gfx.scope.as_ref().and_then(|id| list.iter().position(|(x, _)| x == id)).map_or(0, |i| i as i32 + 1);
    g.set_scope(i);
    g.set_scene_scope(i > 0);
}

/// Switch to the channel graphics (`None`) or scene `id`, dropping the
/// draft of the previous one.
pub fn set(c: &mut Ctx, id: Option<String>) {
    if c.gfx.scope == id && c.gfx.draft.is_some() {
        return;
    }
    let fonts = std::mem::take(&mut c.gfx.fonts);
    let templates = std::mem::take(&mut c.gfx.templates);
    let items = c.gfx.items.clone();
    c.gfx = super::GraphicsState { scope: id, fonts, templates, items, ..Default::default() };
    if c.section() == "graphics" {
        super::opened(c);
    } else {
        show(c);
    }
}

fn pick(c: &mut Ctx, i: i32) {
    let id = usize::try_from(i - 1).ok().and_then(|i| scenes(c).get(i).map(|(id, _)| id.clone()));
    if id == c.gfx.scope {
        return;
    }
    if c.gfx.dirty() {
        let back = c.gfx.scope.clone();
        let id2 = id.clone();
        c.ask("Сохранить изменения графики?", "Изменения графики ещё не сохранены и пропадут при переключении.", "Не сохранять", move |c| set(c, id2));
        // the select shows the old scope until the answer
        c.gfx.scope = back;
        show(c);
        return;
    }
    set(c, id);
}

/// Open the editor on scene `id` (from the scenes screen).
pub fn open_scene(c: &mut Ctx, id: String) {
    if c.gfx.dirty() && c.gfx.scope.as_ref() != Some(&id) {
        c.ask("Сохранить изменения графики?", "В редакторе графики есть несохранённые изменения, они пропадут.", "Не сохранять", move |c| {
            set(c, Some(id.clone()));
            super::super::shell::navigate(c, "graphics");
        });
        return;
    }
    set(c, Some(id));
    super::super::shell::navigate(c, "graphics");
}

/// Save `cfg` into the current scope.
pub fn save(c: &mut Ctx, cfg: GraphicsConfig) {
    if c.gfx.scope.is_none() {
        c.save_doc("/graphics", cfg, "Графика");
        return;
    }
    // the latest copy of the scene: only its graphics change
    let Some(mut item) = scene(c).cloned() else {
        c.toast("Сцена не найдена в медиатеке", true);
        return;
    };
    if let Some(s) = item.scene.as_mut() {
        s.graphics = cfg;
    }
    let ch = c.model.ch();
    let name = if item.title.is_empty() { item.id.clone() } else { item.title.clone() };
    c.change(move |api| {
        api.put(&format!("/channels/{ch}/media/{}", item.id), &item)?;
        Ok(format!("Графика сцены «{name}» сохранена"))
    });
}

/// The library reloaded: a scene's graphics may have changed.
pub fn library_changed(c: &mut Ctx) {
    show(c);
    if c.gfx.scope.is_some() {
        super::reloaded(c);
    }
}

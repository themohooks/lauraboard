//! Screen graphics of the selected scene. The scene keeps its own
//! elements and rules (edited in the graphics editor with the scene picked
//! as the scope); the channel graphics show over it unless switched off.
//! Rules of the channel graphics limited to the scene by its media id (the
//! way scenes had graphics before) are listed too, to switch or remove.

use slint::ComponentHandle;

use lb_core::graphics::{GraphicRule, GraphicsConfig};

use super::{Ctx, touched, vec_model, with};
use crate::shell::{SceneGfxRow, Scenes};

pub fn connect(g: &Scenes) {
    g.on_gfx_toggle(|i| {
        with(move |c| {
            change(c, "Правило показа обновлено", |cfg| {
                if let Some(r) = cfg.rules.get_mut(i as usize) {
                    r.enabled = !r.enabled;
                }
            })
        })
    });
    g.on_gfx_remove(|i| {
        with(move |c| {
            change(c, "Правило показа удалено", |cfg| {
                if (i as usize) < cfg.rules.len() {
                    cfg.rules.remove(i as usize);
                }
            })
        })
    });
    g.on_gfx_channel_toggled(|on| {
        with(move |c| {
            if let Some(s) = c.scenes.draft.as_mut().and_then(|m| m.scene.as_mut()) {
                s.channel_graphics = on;
                touched(c);
            }
        })
    });
    g.on_open_graphics(|| {
        with(|c| {
            if let Some(id) = c.scenes.selected.clone() {
                super::super::graphics::scope::open_scene(c, id);
            }
        })
    });
}

/// The rule of the channel graphics shows only on `scene`.
pub fn for_scene(r: &GraphicRule, scene: &str) -> bool {
    r.conditions.media.iter().any(|m| m == scene)
}

pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Scenes>();
    let cfg = &c.model.graphics;
    let rows: Vec<SceneGfxRow> = match &c.scenes.selected {
        Some(id) => cfg
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| for_scene(r, id))
            .map(|(i, r)| SceneGfxRow {
                index: i as i32,
                name: if r.name.is_empty() { r.id.clone() } else { r.name.clone() }.into(),
                element: cfg.element(&r.element).map(|e| if e.name.is_empty() { e.id.clone() } else { e.name.clone() }).unwrap_or_else(|| r.element.clone()).into(),
                enabled: r.enabled,
            })
            .collect(),
        None => vec![],
    };
    g.set_gfx(vec_model(rows));
    let scene = c.scenes.draft.as_ref().and_then(|m| m.scene.as_ref());
    g.set_gfx_channel(scene.is_none_or(|s| s.channel_graphics));
    // the saved graphics (edited elsewhere, not part of the form)
    let own = c.scenes.selected.as_ref().and_then(|id| c.model.library.iter().find(|m| &m.id == id)).and_then(|m| m.scene.as_ref()).map(|s| &s.graphics);
    g.set_gfx_summary(summary(own).into());
}

fn summary(g: Option<&GraphicsConfig>) -> String {
    let Some(g) = g.filter(|g| !g.elements.is_empty()) else { return "Своей графики у сцены нет.".into() };
    let on = g.rules.iter().filter(|r| r.enabled).count();
    format!("Своя графика: элементов — {}, правил показа — {on}.", g.elements.len())
}

/// Change the channel graphics and save them.
fn change(c: &mut Ctx, done: &'static str, f: impl FnOnce(&mut GraphicsConfig)) {
    // the editor's unsaved draft of the channel graphics would overwrite it
    if c.gfx.scope.is_none() && c.gfx.dirty() {
        c.toast("Сначала сохраните или сбросьте изменения на экране графики", true);
        return;
    }
    let mut cfg = c.model.graphics.clone();
    f(&mut cfg);
    c.model.graphics = cfg.clone();
    show(c);
    c.save_doc("/graphics", cfg, done);
}

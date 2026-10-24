//! Graphics rules limited to the selected scene (by its media id). They
//! are ordinary rules of the channel graphics, saved right away.

use serde_json::json;
use slint::ComponentHandle;

use lb_core::graphics::{GraphicRule, GraphicsConfig};

use super::{Ctx, vec_model, with};
use crate::shell::{SceneGfxRow, Scenes};

pub fn connect(g: &Scenes) {
    g.on_gfx_add(|i| with(move |c| add(c, i as usize)));
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
    g.on_open_graphics(|| with(|c| super::super::shell::navigate(c, "graphics")));
}

fn scene_id(c: &Ctx) -> Option<String> {
    c.scenes.selected.clone()
}

/// The rule shows only on `scene`.
pub fn for_scene(r: &GraphicRule, scene: &str) -> bool {
    r.conditions.media.iter().any(|m| m == scene)
}

pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Scenes>();
    let cfg = &c.model.graphics;
    let names: Vec<slint::SharedString> = cfg.elements.iter().map(|e| if e.name.is_empty() { e.id.clone() } else { e.name.clone() }.into()).collect();
    g.set_element_names(vec_model(names));
    let rows: Vec<SceneGfxRow> = match scene_id(c) {
        Some(id) => cfg
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| for_scene(r, &id))
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
}

/// Change the channel graphics and save them.
fn change(c: &mut Ctx, done: &'static str, f: impl FnOnce(&mut GraphicsConfig)) {
    let mut cfg = c.model.graphics.clone();
    f(&mut cfg);
    c.model.graphics = cfg.clone();
    show(c);
    c.save_doc("/graphics", cfg, done);
}

/// A rule showing element `i` (of the graphics) on the selected scene.
pub fn rule_for(cfg: &GraphicsConfig, element: &str, scene: &str, scene_title: &str) -> Option<GraphicRule> {
    let mut n = 1;
    while cfg.rules.iter().any(|r| r.id == format!("{scene}-{n}")) {
        n += 1;
    }
    let el = cfg.element(element)?;
    let name = format!("{scene_title}: {}", if el.name.is_empty() { &el.id } else { &el.name });
    serde_json::from_value(json!({
        "id": format!("{scene}-{n}"),
        "name": name,
        "element": element,
        "priority": 10,
        "conditions": { "media": [scene] },
    }))
    .ok()
}

fn add(c: &mut Ctx, i: usize) {
    let Some(id) = scene_id(c) else { return };
    let title = c.scenes.draft.as_ref().map(super::title).unwrap_or_else(|| id.clone());
    let Some(el) = c.model.graphics.elements.get(i).map(|e| e.id.clone()) else { return };
    let Some(rule) = rule_for(&c.model.graphics, &el, &id, &title) else { return };
    change(c, "Графика сцены добавлена", move |cfg| cfg.rules.push(rule));
}

//! Logical layers of the graphics editor: the layer panel groups elements
//! under named layers (collapsible, switched on / off for the air,
//! exclusive, moved up / down, renamed, added, removed) and filters
//! them: all, common graphics, graphics of scenes.

use std::collections::HashSet;

use slint::{ComponentHandle, SharedString};

use lb_core::graphics::{GraphicsConfig, Layer, preset_layer};

use super::{active, kind_id, label, mutate, refresh, s, with};
use crate::bridge::{Ctx, vec_model};
use crate::shell::{AppWindow, Gfx, LayerRow};

/// Layer panel filter.
const ALL: i32 = 0;
const COMMON: i32 = 1;
const SCENES: i32 = 2;

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Gfx>();
    g.on_layer_collapse(|id| {
        with(move |c| {
            if !c.gfx.collapsed.remove(&id) {
                c.gfx.collapsed.insert(id);
            }
            rows(c);
        })
    });
    g.on_layer_toggle_enabled(|id| with(move |c| edit_layer(c, id, |l| l.enabled = !l.enabled)));
    g.on_layer_toggle_exclusive(|id| with(move |c| edit_layer(c, id, |l| l.exclusive = !l.exclusive)));
    g.on_layer_rename(|id, name| {
        let name = name.trim().to_string();
        with(move |c| {
            if let Some(ui) = c.window() {
                ui.global::<Gfx>().set_layer_editing(i32::MIN);
            }
            if !name.is_empty() {
                edit_layer(c, id, move |l| l.name = name);
            }
        })
    });
    g.on_layer_move(|id, d| with(move |c| move_layer(c, id, d)));
    g.on_layer_add(|| with(add_layer));
    g.on_layer_remove(|id| with(move |c| remove_layer(c, id)));
    g.on_layer_filter_changed(|| with(rows));
    g.on_layer_step(|i, d| with(move |c| step_element(c, i as usize, d)));
    g.on_layer_organize(|| with(organize));
}

/// Ids of the library items that are scenes.
fn scene_ids(c: &Ctx) -> HashSet<String> {
    c.model.library.iter().filter(|m| m.is_scene()).map(|m| m.id.clone()).collect()
}

/// Elements shown only on scenes: every rule of theirs is limited to
/// scenes (by media id), and there is one.
pub fn scene_elements(cfg: &GraphicsConfig, scenes: &HashSet<String>) -> HashSet<String> {
    let mut out = HashSet::new();
    for e in &cfg.elements {
        let rules: Vec<_> = cfg.rules.iter().filter(|r| r.element == e.id).collect();
        let only_scenes = !rules.is_empty()
            && rules.iter().all(|r| !r.conditions.media.is_empty() && r.conditions.media.iter().all(|m| scenes.contains(m)));
        if only_scenes {
            out.insert(e.id.clone());
        }
    }
    out
}

/// The layer panel and the layer choices of the properties panel.
pub fn rows(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let cfg = c.gfx.draft.clone().unwrap_or_default();
    let on = active(c);
    let filter = g.get_layer_filter();
    let scene_only = scene_elements(&cfg, &scene_ids(c));
    let shown = |id: &str| match filter {
        COMMON => !scene_only.contains(id),
        SCENES => scene_only.contains(id),
        _ => true,
    };
    let ids = cfg.layer_ids();
    let mut out: Vec<LayerRow> = Vec::new();
    for &lid in &ids {
        let layer = cfg.layer(lid).cloned().unwrap_or_else(|| Layer::new(lid, ""));
        // top of the layer first, like the canvas draws it
        let members: Vec<usize> = (0..cfg.elements.len()).rev().filter(|&i| cfg.elements[i].layer == lid && shown(&cfg.elements[i].id)).collect();
        if filter != ALL && members.is_empty() {
            continue;
        }
        let collapsed = c.gfx.collapsed.contains(&lid);
        out.push(LayerRow {
            header: true,
            index: -1,
            name: s(cfg.layer_name(lid)),
            kind: SharedString::new(),
            kind_name: SharedString::new(),
            layer: lid,
            hidden: false,
            locked: false,
            on_air: members.iter().any(|&i| on.contains(&cfg.elements[i].id)),
            count: members.len() as i32,
            enabled: layer.enabled,
            exclusive: layer.exclusive,
            collapsed,
            scene: false,
        });
        if collapsed {
            continue;
        }
        for i in members {
            let e = &cfg.elements[i];
            out.push(LayerRow {
                header: false,
                index: i as i32,
                name: s(label(e)),
                kind: s(kind_id(&e.kind)),
                kind_name: s(super::super::air::element_kind_name(&e.kind)),
                layer: lid,
                hidden: c.gfx.hidden.contains(&e.id),
                locked: c.gfx.locked.contains(&e.id),
                on_air: on.contains(&e.id),
                count: 0,
                enabled: layer.enabled,
                exclusive: false,
                collapsed: false,
                scene: scene_only.contains(&e.id),
            });
        }
    }
    g.set_layers(vec_model(out));
    g.set_layer_names(vec_model(ids.iter().map(|id| s(format!("{} · {id}", cfg.layer_name(*id)))).collect()));
    c.gfx.layer_ids = ids;
}

/// Index of the selected element's layer in `layer-names`.
pub fn layer_index(c: &Ctx, layer: i32) -> i32 {
    c.gfx.layer_ids.iter().position(|l| *l == layer).map(|i| i as i32).unwrap_or(-1)
}

/// Layer id at `index` of `layer-names`.
pub fn layer_at(c: &Ctx, index: i32) -> Option<i32> {
    usize::try_from(index).ok().and_then(|i| c.gfx.layer_ids.get(i)).copied()
}

/// Change layer `id` (its entry is made when it has none).
fn edit_layer(c: &mut Ctx, id: i32, f: impl FnOnce(&mut Layer)) {
    mutate(c, None, move |cfg| {
        if cfg.layer(id).is_none() {
            let name = cfg.layer_name(id);
            cfg.layers.push(Layer::new(id, name));
        }
        if let Some(l) = cfg.layers.iter_mut().find(|l| l.id == id) {
            f(l);
        }
    });
    refresh(c);
}

/// Give layers `a` and `b` each other's height.
fn swap_layers(cfg: &mut GraphicsConfig, a: i32, b: i32) {
    let other = |v: i32| if v == a { b } else if v == b { a } else { v };
    for e in &mut cfg.elements {
        e.layer = other(e.layer);
    }
    for r in &mut cfg.rules {
        r.layer = r.layer.map(other);
    }
    for l in &mut cfg.layers {
        l.id = other(l.id);
    }
}

/// Move layer `id` above (`d` > 0) or below the next one.
fn move_layer(c: &mut Ctx, id: i32, d: i32) {
    let ids = c.gfx.draft.as_ref().map(|cfg| cfg.layer_ids()).unwrap_or_default();
    let Some(pos) = ids.iter().position(|l| *l == id) else { return };
    // the list runs top first: "up" is the previous entry
    let Some(&next) = (if d > 0 { pos.checked_sub(1).and_then(|p| ids.get(p)) } else { ids.get(pos + 1) }) else { return };
    mutate(c, None, move |cfg| swap_layers(cfg, id, next));
    // folding follows the layer to its new height
    let set = &mut c.gfx.collapsed;
    let (was_a, was_b) = (set.remove(&id), set.remove(&next));
    if was_a {
        set.insert(next);
    }
    if was_b {
        set.insert(id);
    }
    refresh(c);
}

fn add_layer(c: &mut Ctx) {
    let top = c.gfx.draft.as_ref().and_then(|cfg| cfg.layer_ids().first().copied()).unwrap_or(0);
    let id = top + 10;
    mutate(c, None, move |cfg| cfg.layers.push(Layer::new(id, "Новый слой")));
    refresh(c);
    if let Some(ui) = c.window() {
        ui.global::<Gfx>().set_layer_editing(id);
    }
}

/// Remove an empty layer.
fn remove_layer(c: &mut Ctx, id: i32) {
    let used = c.gfx.draft.as_ref().is_some_and(|cfg| cfg.elements.iter().any(|e| e.layer == id));
    if used {
        c.toast("В слое есть элементы — сначала перенесите их в другие слои", true);
        return;
    }
    mutate(c, None, move |cfg| cfg.layers.retain(|l| l.id != id));
    refresh(c);
}

/// Move element `i` to the layer above / below its own.
fn step_element(c: &mut Ctx, i: usize, d: i32) {
    let Some(cfg) = c.gfx.draft.as_ref() else { return };
    let Some(cur) = cfg.elements.get(i).map(|e| e.layer) else { return };
    let ids = cfg.layer_ids();
    let Some(pos) = ids.iter().position(|l| *l == cur) else { return };
    let target = if d > 0 { pos.checked_sub(1).and_then(|p| ids.get(p)).copied() } else { ids.get(pos + 1).copied() };
    let Some(target) = target else {
        c.toast(if d > 0 { "Это самый верхний слой" } else { "Это самый нижний слой" }, false);
        return;
    };
    mutate(c, None, move |cfg| {
        if let Some(e) = cfg.elements.get_mut(i) {
            e.layer = target;
        }
    });
    c.gfx.collapsed.remove(&target);
    refresh(c);
}

/// Every element to the preset layer of its kind (graphics of scenes to
/// "Сцены"); layers left empty and unnamed go away.
pub fn organize_config(cfg: &mut GraphicsConfig, scene_only: &HashSet<String>) {
    for e in &mut cfg.elements {
        let (id, _) = if scene_only.contains(&e.id) { lb_core::graphics::LAYER_PRESETS[4] } else { preset_layer(&e.kind) };
        e.layer = id;
    }
    // rule overrides would put them back where they were
    for r in &mut cfg.rules {
        r.layer = None;
    }
    for (id, name) in lb_core::graphics::LAYER_PRESETS {
        if cfg.elements.iter().any(|e| e.layer == id) && cfg.layer(id).is_none() {
            cfg.layers.push(Layer::new(id, name));
        }
    }
    let used: HashSet<i32> = cfg.elements.iter().map(|e| e.layer).collect();
    cfg.layers.retain(|l| used.contains(&l.id) || !l.name.trim().is_empty() && !lb_core::graphics::LAYER_PRESETS.iter().any(|p| p.0 == l.id));
    cfg.layers.sort_by_key(|l| l.id);
}

fn organize(c: &mut Ctx) {
    let scenes = scene_ids(c);
    let go = move |c: &mut Ctx| {
        mutate(c, None, |cfg| {
            let only = scene_elements(cfg, &scenes);
            organize_config(cfg, &only);
        });
        c.gfx.collapsed.clear();
        refresh(c);
        c.toast_with("Элементы разложены по слоям — не забудьте сохранить", "Отменить", super::undo);
    };
    c.ask(
        "Разложить элементы по слоям?",
        "Каждый элемент попадёт в слой своего типа: «Фон и подложки», «Плашки и титры», «Бегущие строки», «Логотип, часы и таймеры»; графика, которая показывается только на сценах, — в «Сцены». Слои из правил показа сбрасываются. Отменить можно кнопкой «Отменить».",
        "Разложить",
        go,
    );
}

/// Layer for a new element of `kind`: the "Сцены" filter puts it with
/// the scenes; otherwise the preset of its kind. The entry is made if the
/// layer is new.
pub fn layer_for_new(c: &Ctx, cfg: &mut GraphicsConfig, kind: &lb_core::graphics::ElementKind) -> i32 {
    let scenes = c.window().is_some_and(|ui| ui.global::<Gfx>().get_layer_filter() == SCENES);
    let (id, name) = if scenes { lb_core::graphics::LAYER_PRESETS[4] } else { preset_layer(kind) };
    if cfg.layer(id).is_none() && !cfg.elements.iter().any(|e| e.layer == id) {
        cfg.layers.push(Layer::new(id, name));
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::graphics::{Element, GraphicRule};

    fn el(id: &str, layer: i32) -> Element {
        serde_json::from_value(serde_json::json!({ "id": id, "type": "text", "text": "x", "layer": layer })).unwrap()
    }

    fn rule(id: &str, element: &str, media: &[&str]) -> GraphicRule {
        serde_json::from_value(serde_json::json!({ "id": id, "element": element, "conditions": { "media": media }, "layer": 1 })).unwrap()
    }

    #[test]
    fn swapping_layers_moves_elements_rules_and_entries() {
        let mut cfg = GraphicsConfig { layers: vec![Layer::new(1, "низ"), Layer::new(2, "верх")], elements: vec![el("a", 1), el("b", 2)], ..Default::default() };
        cfg.rules.push(rule("r", "a", &[]));
        swap_layers(&mut cfg, 1, 2);
        assert_eq!((cfg.elements[0].layer, cfg.elements[1].layer), (2, 1));
        assert_eq!(cfg.rules[0].layer, Some(2));
        assert_eq!(cfg.layer_name(2), "низ");
    }

    #[test]
    fn organizing_groups_by_kind_and_drops_empty_layers() {
        let mut cfg = GraphicsConfig {
            layers: vec![Layer::new(3, ""), Layer::new(77, "Мой слой")],
            elements: vec![el("a", 3), el("b", 4), el("c", 5)],
            ..Default::default()
        };
        let only: HashSet<String> = ["c".to_string()].into();
        organize_config(&mut cfg, &only);
        assert_eq!(cfg.elements.iter().map(|e| e.layer).collect::<Vec<_>>(), [20, 20, 50]);
        assert_eq!(cfg.layers.iter().map(|l| l.id).collect::<Vec<_>>(), [20, 50, 77], "named layers stay");
        assert_eq!(cfg.layer_name(20), "Плашки и титры");
    }

    #[test]
    fn scene_elements_are_those_only_scenes_use() {
        let scenes: HashSet<String> = ["clock".to_string()].into();
        let cfg = GraphicsConfig {
            elements: vec![el("big-clock", 1), el("logo", 1), el("mixed", 1)],
            rules: vec![rule("1", "big-clock", &["clock"]), rule("2", "logo", &[]), rule("3", "mixed", &["clock"]), rule("4", "mixed", &["film"])],
            ..Default::default()
        };
        let s = scene_elements(&cfg, &scenes);
        assert!(s.contains("big-clock") && !s.contains("logo") && !s.contains("mixed"));
    }
}

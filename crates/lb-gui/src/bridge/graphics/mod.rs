//! On-screen graphics editor: canvas (drag / resize with snapping),
//! layers, properties, exact server render and animation preview, live
//! control, display rules, template variables, saved design templates.
//!
//! * [`design`] — shape design fields and plate text lines
//! * [`fonts`] — font picker (server fonts, files, Google Fonts)
//! * [`textures`] — ready-made plate textures
//! * [`layers`] — logical layers: the grouped layer panel and its filter
//! * [`plate_editor`] — the advanced plate editor (preview, styles)
//! * [`scope`] — the channel graphics or the own graphics of a scene

mod design;
mod fonts;
mod layers;
pub(crate) mod plate_editor;
pub mod scope;
mod textures;

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::json;
use slint::{Color, ComponentHandle, Image, Model, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use lb_core::Dur;
use lb_core::graphics::{
    AnimKind, Animation, CountdownTarget, Element, ElementKind, FillDirection, GraphicRule, GraphicsConfig, Placement, ProgressTarget,
    TextStyle, TimeFormat,
};

use super::{Ctx, vec_model, with};
use crate::fmt::KINDS;
use crate::logic::graphics_preview::edit_preview;
use crate::logic::placement::{self, ANCHORS, Rect};
use crate::logic::undo::History;
use crate::net::Msg;
use crate::shell::{App, AppWindow, CanvasItem, GRuleRow, Gfx, RuleTextRow, VarRow};

struct Drag {
    index: usize,
    handle: i32,
    start: Rect,
    /// Frame or the box of the element's group.
    region: Rect,
    before: GraphicsConfig,
}

#[derive(Default)]
pub struct GraphicsState {
    saved: Option<GraphicsConfig>,
    draft: Option<GraphicsConfig>,
    history: History<GraphicsConfig>,
    selected: Option<usize>,
    hidden: HashSet<String>,
    locked: HashSet<String>,
    drag: Option<Drag>,
    items: Rc<VecModel<CanvasItem>>,
    /// Identifies the pending throttle timer, including across scope switches.
    render_scheduled: u64,
    render_ticket: u64,
    preview_layer: Option<String>,
    pending_web: bool,
    force_render: bool,
    /// A server render is running; `render_again`: the draft changed meanwhile.
    rendering: bool,
    render_again: bool,
    last_edit: Option<(String, Instant)>,
    templates: Vec<PathBuf>,
    /// Selected text line of a plate.
    field: usize,
    fonts: fonts::FontList,
    /// Folded layers of the layer panel.
    collapsed: HashSet<i32>,
    /// Layer ids in the order of `layer-names` (top first).
    layer_ids: Vec<i32>,
    /// Scene whose own graphics are edited (`None`: the channel's).
    pub(crate) scope: Option<String>,
}

impl GraphicsState {
    pub fn dirty(&self) -> bool {
        self.draft.is_some() && self.draft != self.saved
    }
}

const ANIMS: [AnimKind; 23] = AnimKind::ALL;

fn s(v: impl Into<SharedString>) -> SharedString {
    v.into()
}

/// Output frame size (channel video format; 1920×1080 when unknown).
fn frame(c: &Ctx) -> (f32, f32) {
    let v = c.channel_config.as_ref().map(|c| &c["video"]);
    match v.and_then(|v| Some((v["width"].as_f64()?, v["height"].as_f64()?))) {
        Some((w, h)) if w > 0.0 && h > 0.0 => (w as f32, h as f32),
        _ => (1920.0, 1080.0),
    }
}

fn kind_id(k: &ElementKind) -> &'static str {
    match k {
        ElementKind::Text { .. } => "text",
        ElementKind::Ticker { .. } => "ticker",
        ElementKind::Clock { .. } => "clock",
        ElementKind::Countdown { .. } => "countdown",
        ElementKind::Image { .. } => "image",
        ElementKind::Rect { .. } => "rect",
        ElementKind::Group { .. } => "group",
        ElementKind::Plate { .. } => "plate",
        ElementKind::Progress { .. } => "progress",
    }
}

const TARGETS: [CountdownTarget; 3] = [CountdownTarget::ItemRemaining, CountdownTarget::NextProgram, CountdownTarget::ProgramRemaining];

/// Chips of the excluded content kinds (KINDS order).
fn exclude_flags(v: &[lb_core::media::MediaKind]) -> slint::ModelRc<bool> {
    vec_model(KINDS.iter().map(|k| v.contains(k)).collect())
}

fn kind_index(k: &ElementKind) -> i32 {
    match k {
        ElementKind::Text { .. } => 0,
        ElementKind::Ticker { .. } => 1,
        ElementKind::Clock { .. } => 2,
        ElementKind::Countdown { .. } => 3,
        ElementKind::Image { .. } => 4,
        ElementKind::Rect { .. } => 5,
        ElementKind::Group { .. } => 6,
        ElementKind::Plate { .. } => 7,
        ElementKind::Progress { .. } => 8,
    }
}

fn label(e: &Element) -> String {
    if e.name.is_empty() { e.id.clone() } else { e.name.clone() }
}

/// `#RRGGBB[AA]` → colour for swatches.
pub(crate) fn color_of(hex: &str) -> Color {
    if !placement::valid_color(hex) {
        return Color::from_argb_u8(0, 0, 0, 0);
    }
    let h = hex.trim().trim_start_matches('#');
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    Color::from_argb_u8(if h.len() == 8 { b(6) } else { 255 }, b(0), b(2), b(4))
}

/// Colour → `#RRGGBB`, or `#RRGGBBAA` when translucent.
fn hex_of(c: Color) -> String {
    let base = format!("#{:02X}{:02X}{:02X}", c.red(), c.green(), c.blue());
    if c.alpha() == 255 { base } else { format!("{base}{:02X}", c.alpha()) }
}

pub fn connect(ui: &AppWindow) {
    design::connect(ui);
    fonts::connect(ui);
    layers::connect(ui);
    plate_editor::connect(ui);
    textures::connect(ui);
    let g = ui.global::<Gfx>();
    scope::connect(&g);
    g.on_color_hex(|c| hex_of(c).into());
    g.on_parse_color(|s| color_of(&s));
    g.on_select(|i| {
        with(move |c| {
            let sel = usize::try_from(i).ok();
            if sel != c.gfx.selected {
                c.gfx.field = 0;
            }
            let changed = c.gfx.selected != sel;
            c.gfx.selected = sel;
            if changed {
                invalidate_layers(c);
            }
            props(c);
            if let Some(ui) = c.window() {
                ui.global::<Gfx>().set_selected(i);
            }
            if changed {
                render(c);
            }
        })
    });
    g.on_drag_start(|i, h| with(move |c| drag_start(c, i as usize, h)));
    g.on_drag_move(|i, h, dx, dy| with(move |c| drag_move(c, i as usize, h, dx, dy)));
    g.on_drag_end(|| with(drag_end));
    g.on_nudge(|dx, dy| with(move |c| nudge(c, dx, dy)));
    g.on_props_changed(|| with(props_changed));
    g.on_insert_var(|v| {
        with(move |c| {
            if let Some(ui) = c.window() {
                let g = ui.global::<Gfx>();
                g.set_p_text(append_var(&g.get_p_text(), &v).into());
            }
            props_changed(c);
            // not typed: the plate's line list may show the new text too
            props(c);
        })
    });
    g.on_pick_image(|| with(pick_image));
    g.on_add(|k| with(move |c| add(c, k.as_str())));
    g.on_duplicate(|| with(duplicate));
    g.on_remove(|| with(remove));
    g.on_toggle_hidden(|i| with(move |c| toggle_flag(c, i as usize, true)));
    g.on_toggle_locked(|i| with(move |c| toggle_flag(c, i as usize, false)));
    g.on_toggle_exclude(|i| with(move |c| toggle_exclude(c, i as usize)));
    g.on_live(|a| with(move |c| live(c, a.as_str())));
    g.on_render(|| {
        with(|c| {
            c.gfx.force_render = true;
            render(c);
        })
    });
    g.on_preview_animation(|| with(preview_animation));
    g.on_rule_edited(|i, f, v| with(move |c| rule_edited(c, i as usize, f.as_str(), v.as_str())));
    g.on_rule_add(|| with(rule_add));
    g.on_rule_insert_var(|i, k, v| with(move |c| rule_insert_var(c, i as usize, k.max(0) as usize, &v)));
    g.on_rule_remove(|i| {
        with(move |c| {
            mutate(c, None, |cfg| {
                if (i as usize) < cfg.rules.len() {
                    cfg.rules.remove(i as usize);
                }
            });
            refresh(c);
            c.toast_with("Правило показа удалено", "Отменить", undo);
        })
    });
    g.on_var_edited(|i, v| with(move |c| var_edited(c, i as usize, v.to_string())));
    g.on_var_add(|| with(var_add));
    g.on_var_remove(|i| {
        with(move |c| {
            let Some(key) = sorted_vars(c).get(i as usize).map(|(k, _)| k.clone()) else { return };
            mutate(c, None, |cfg| {
                cfg.vars.remove(&key);
            });
            refresh(c);
        })
    });
    g.on_template_save(|| with(template_save));
    g.on_template_apply(|i| with(move |c| template_apply(c, i as usize)));
    g.on_template_delete(|i| with(move |c| template_delete(c, i as usize)));
    g.on_save(|| with(save));
    g.on_revert(|| {
        with(|c| {
            let saved = c.gfx.saved.clone();
            mutate(c, None, move |cfg| {
                if let Some(s) = saved {
                    *cfg = s;
                }
            });
            refresh(c);
        })
    });
    g.on_undo(|| with(undo));
    g.on_redo(|| with(redo));
}

pub fn reloaded(c: &mut Ctx) {
    let was_dirty = c.gfx.dirty();
    let source = scope::source(c);
    let changed = c.gfx.saved.as_ref() != Some(&source);
    c.gfx.saved = Some(source);
    if !was_dirty {
        c.gfx.draft = c.gfx.saved.clone();
    }
    if c.section() == "graphics" {
        refresh(c);
        if changed && !was_dirty {
            render(c);
        }
    }
}

/// Select element `i` (as a click on the canvas would).
#[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
pub fn select(c: &mut Ctx, i: usize) {
    let changed = c.gfx.selected != Some(i);
    c.gfx.selected = Some(i);
    if changed {
        invalidate_layers(c);
    }
    props(c);
    if let Some(ui) = c.window() {
        ui.global::<Gfx>().set_selected(i as i32);
    }
    if changed {
        render(c);
    }
}

pub fn opened(c: &mut Ctx) {
    if c.gfx.draft.is_none() {
        c.gfx.saved = Some(scope::source(c));
        c.gfx.draft = c.gfx.saved.clone();
    }
    refresh(c);
    render(c);
}

fn mutate(c: &mut Ctx, coalesce: Option<&str>, f: impl FnOnce(&mut GraphicsConfig)) {
    let Some(before) = c.gfx.draft.clone() else { return };
    let mut after = before.clone();
    f(&mut after);
    let merge = match (coalesce, &c.gfx.last_edit) {
        (Some(k), Some((lk, at))) => k == lk && at.elapsed() < Duration::from_millis(1500),
        _ => false,
    };
    if !merge {
        c.gfx.history.record(&before, &after);
    }
    c.gfx.last_edit = coalesce.map(|k| (k.to_string(), Instant::now()));
    c.gfx.draft = Some(after);
    schedule_render(c);
}

fn flags(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    g.set_dirty(c.gfx.dirty());
    g.set_can_undo(c.gfx.history.can_undo());
    g.set_can_redo(c.gfx.history.can_redo());
}

fn active(c: &Ctx) -> Vec<String> {
    c.model.status.graphics.iter().map(|g| g.element.clone()).collect()
}

fn canvas_item(c: &Ctx, i: usize, e: &Element, on: &[String]) -> CanvasItem {
    let (fw, fh) = frame(c);
    let cfg = c.gfx.draft.as_ref();
    let region = cfg.map(|cfg| placement::region_of(cfg, &e.id, fw, fh)).unwrap_or(Rect { x: 0.0, y: 0.0, w: fw, h: fh });
    let r = placement::rect_in(e, region);
    CanvasItem {
        index: i as i32,
        name: s(label(e)),
        kind: s(kind_id(&e.kind)),
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        hidden: c.gfx.hidden.contains(&e.id),
        locked: c.gfx.locked.contains(&e.id),
        on_air: on.contains(&e.id),
    }
}

/// Everything except the property fields.
pub fn refresh(c: &mut Ctx) {
    scope::show(c);
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let cfg = c.gfx.draft.clone().unwrap_or_default();
    let (fw, fh) = frame(c);
    g.set_frame_w(fw);
    g.set_frame_h(fh);
    let on = active(c);
    // drawing order: lower layers first
    let mut order: Vec<usize> = (0..cfg.elements.len()).collect();
    order.sort_by_key(|&i| cfg.elements[i].layer);
    let items: Vec<CanvasItem> = order.iter().map(|&i| canvas_item(c, i, &cfg.elements[i], &on)).collect();
    c.gfx.items = Rc::new(VecModel::from(items));
    g.set_items(c.gfx.items.clone().into());
    layers::rows(c);
    if c.gfx.selected.is_some_and(|i| i >= cfg.elements.len()) {
        c.gfx.selected = None;
    }
    g.set_selected(c.gfx.selected.map(|i| i as i32).unwrap_or(-1));
    if !has_edit_layer(c) {
        invalidate_layers(c);
    }
    update_preview_target(c);
    g.set_element_names(vec_model(cfg.elements.iter().map(|e| s(format!("{} ({})", label(e), e.id))).collect()));
    rules_rows(c);
    vars_rows(c);
    templates_list(c);
    props(c);
    flags(c);
}

/// Update one canvas box in place (during drags).
fn update_item(c: &mut Ctx, index: usize) {
    let Some(e) = c.gfx.draft.as_ref().and_then(|d| d.elements.get(index)).cloned() else { return };
    let on = active(c);
    // a group carries its children along: re-place every row then
    let all = matches!(e.kind, ElementKind::Group { .. });
    for r in 0..c.gfx.items.row_count() {
        let Some(row) = c.gfx.items.row_data(r) else { continue };
        if row.index == index as i32 {
            let it = canvas_item(c, index, &e, &on);
            c.gfx.items.set_row_data(r, it);
        } else if all && let Some(other) = c.gfx.draft.as_ref().and_then(|d| d.elements.get(row.index as usize)).cloned() {
            let it = canvas_item(c, row.index as usize, &other, &on);
            c.gfx.items.set_row_data(r, it);
        }
    }
    update_preview_target(c);
}

fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

fn opt_num(v: Option<f32>) -> String {
    v.map(fmt_num).unwrap_or_default()
}

/// Property fields of the selected element.
fn props(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let Some(e) = c.gfx.selected.and_then(|i| c.gfx.draft.as_ref().and_then(|d| d.elements.get(i))).cloned() else {
        g.set_p_has(false);
        return;
    };
    g.set_p_has(true);
    g.set_p_id(s(e.id.clone()));
    g.set_p_name(s(e.name.clone()));
    g.set_p_kind(kind_index(&e.kind));
    g.set_p_anchor(ANCHORS.iter().position(|a| *a == e.placement.anchor).unwrap_or(0) as i32);
    pos_fields(c, &e);
    g.set_p_opacity(e.opacity);
    g.set_p_layer(layers::layer_index(c, e.layer));
    let st = match &e.kind {
        ElementKind::Text { text, style } => {
            g.set_p_text(s(text.clone()));
            Some(style.clone())
        }
        ElementKind::Ticker { text, style, speed, width } => {
            g.set_p_text(s(text.clone()));
            g.set_p_speed(s(fmt_num(*speed)));
            g.set_p_twidth(s(opt_num(*width)));
            Some(style.clone())
        }
        ElementKind::Clock { format, style } => {
            g.set_p_format(s(format.clone()));
            Some(style.clone())
        }
        ElementKind::Countdown { target, style, prefix, exclude, format } => {
            g.set_p_target(TARGETS.iter().position(|t| t == target).unwrap_or(0) as i32);
            g.set_p_prefix(s(prefix.clone()));
            g.set_p_tformat(TimeFormat::ALL.iter().position(|f| f == format).unwrap_or(0) as i32);
            g.set_p_exclude(exclude_flags(exclude));
            Some(style.clone())
        }
        ElementKind::Progress { width, height, color, gradient, track, radius, target, exclude, direction, shape } => {
            g.set_p_color(s(color.clone()));
            g.set_p_rw(s(fmt_num(*width)));
            g.set_p_rh(s(fmt_num(*height)));
            g.set_p_radius(s(fmt_num(*radius)));
            g.set_pg_gradient(s(gradient.clone().unwrap_or_default()));
            g.set_pg_track(s(track.clone()));
            g.set_pg_target(if *target == ProgressTarget::Item { 1 } else { 0 });
            g.set_pg_direction(FillDirection::ALL.iter().position(|d| d == direction).unwrap_or(0) as i32);
            g.set_p_exclude(exclude_flags(exclude));
            g.set_sw_p_color(color_of(color));
            g.set_sw_pg_gradient(color_of(&g.get_pg_gradient()));
            g.set_sw_pg_track(color_of(track));
            design::show_shape(&g, shape);
            None
        }
        ElementKind::Image { path } => {
            g.set_p_path(s(path.clone()));
            None
        }
        ElementKind::Rect { color, width, height, radius, shape } => {
            g.set_p_color(s(color.clone()));
            g.set_sw_p_color(color_of(color));
            g.set_p_rw(s(fmt_num(*width)));
            g.set_p_rh(s(fmt_num(*height)));
            g.set_p_radius(s(fmt_num(*radius)));
            design::show_shape(&g, shape);
            None
        }
        ElementKind::Plate { .. } => design::show_plate(c, &g, &e.kind),
        ElementKind::Group { children } => {
            g.set_p_children(s(children.join(", ")));
            None
        }
    };
    if let Some(st) = st {
        g.set_s_font(s(st.font.clone().unwrap_or_default()));
        g.set_s_bold(st.bold);
        g.set_s_italic(st.italic);
        g.set_s_size(s(fmt_num(st.size)));
        g.set_s_color(s(st.color.clone()));
        g.set_s_outline(s(st.outline_color.clone().unwrap_or_default()));
        g.set_s_outline_w(s(fmt_num(st.outline_width)));
        g.set_s_shadow(s(st.shadow_color.clone().unwrap_or_default()));
        g.set_s_shadow_x(s(fmt_num(st.shadow_offset.0)));
        g.set_s_shadow_y(s(fmt_num(st.shadow_offset.1)));
        g.set_s_bg(s(st.background.clone().unwrap_or_default()));
        g.set_s_padding(s(fmt_num(st.padding)));
        g.set_s_corner(s(fmt_num(st.corner_radius)));
        g.set_s_spacing(s(fmt_num(st.letter_spacing)));
        g.set_s_upper(st.uppercase);
        swatches(c);
    }
    g.set_a_in(ANIMS.iter().position(|a| *a == e.animation.anim_in).unwrap_or(0) as i32);
    g.set_a_out(ANIMS.iter().position(|a| *a == e.animation.anim_out).unwrap_or(0) as i32);
    g.set_a_ms(s(e.animation.duration.ms().to_string()));
    g.set_p_profiles(s(e.profiles.join(", ")));
    g.set_prop_errors(SharedString::new());
}

fn pos_fields(c: &Ctx, e: &Element) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    g.set_p_x(s(fmt_num(e.placement.x)));
    g.set_p_y(s(fmt_num(e.placement.y)));
    g.set_p_w(s(opt_num(e.placement.width)));
    g.set_p_h(s(opt_num(e.placement.height)));
}

fn swatches(c: &Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    g.set_sw_p_color(color_of(&g.get_p_color()));
    g.set_sw_s_color(color_of(&g.get_s_color()));
    g.set_sw_s_outline(color_of(&g.get_s_outline()));
    g.set_sw_s_shadow(color_of(&g.get_s_shadow()));
    g.set_sw_s_bg(color_of(&g.get_s_bg()));
    g.set_sw_pg_gradient(color_of(&g.get_pg_gradient()));
    g.set_sw_pg_track(color_of(&g.get_pg_track()));
    design::swatches(&g);
}

/// Read the property fields into the selected element.
/// `text` with `var` at the end, a space between them unless there is one.
fn append_var(text: &str, var: &str) -> String {
    if text.is_empty() || text.ends_with(char::is_whitespace) { format!("{text}{var}") } else { format!("{text} {var}") }
}

fn props_changed(c: &mut Ctx) {
    let Some(i) = c.gfx.selected else { return };
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let Some(mut e) = c.gfx.draft.as_ref().and_then(|d| d.elements.get(i)).cloned() else { return };
    let errors = std::cell::RefCell::new(Vec::<String>::new());
    let mut num = |label: &str, v: SharedString, old: f32| -> f32 {
        match v.trim().replace(',', ".").parse::<f32>() {
            Ok(x) if x.is_finite() => x,
            _ => {
                errors.borrow_mut().push(format!("{label}: нужно число"));
                old
            }
        }
    };
    e.name = g.get_p_name().trim().to_string();
    e.placement.anchor = ANCHORS.get(g.get_p_anchor().max(0) as usize).copied().unwrap_or_default();
    e.placement.x = num("X", g.get_p_x(), e.placement.x);
    e.placement.y = num("Y", g.get_p_y(), e.placement.y);
    e.placement.width = if g.get_p_w().trim().is_empty() {
        None
    } else {
        Some(num("Ширина", g.get_p_w(), e.placement.width.unwrap_or(0.0)).max(1.0))
    };
    e.placement.height = if g.get_p_h().trim().is_empty() {
        None
    } else {
        Some(num("Высота", g.get_p_h(), e.placement.height.unwrap_or(0.0)).max(1.0))
    };
    e.opacity = g.get_p_opacity().clamp(0.0, 1.0);
    if let Some(l) = layers::layer_at(c, g.get_p_layer()) {
        e.layer = l;
    }
    let mut color = |label: &str, v: SharedString, required: bool| -> Option<String> {
        let t = v.trim().to_string();
        if t.is_empty() && !required {
            return None;
        }
        if placement::valid_color(&t) {
            Some(t)
        } else {
            errors.borrow_mut().push(format!("{label}: цвет в формате #RRGGBB или #RRGGBBAA"));
            None
        }
    };
    let style_from = |st: &TextStyle,
                      g: &Gfx,
                      color: &mut dyn FnMut(&str, SharedString, bool) -> Option<String>,
                      num: &mut dyn FnMut(&str, SharedString, f32) -> f32|
     -> TextStyle {
        TextStyle {
            font: Some(g.get_s_font().trim().to_string()).filter(|x| !x.is_empty()),
            bold: g.get_s_bold(),
            italic: g.get_s_italic(),
            size: num("Размер", g.get_s_size(), st.size).clamp(4.0, 400.0),
            color: color("Цвет текста", g.get_s_color(), true).unwrap_or_else(|| st.color.clone()),
            outline_color: color("Обводка", g.get_s_outline(), false),
            outline_width: num("Толщина обводки", g.get_s_outline_w(), st.outline_width).max(0.0),
            shadow_color: color("Тень", g.get_s_shadow(), false),
            shadow_offset: (num("Тень dx", g.get_s_shadow_x(), st.shadow_offset.0), num("Тень dy", g.get_s_shadow_y(), st.shadow_offset.1)),
            background: color("Подложка", g.get_s_bg(), false),
            padding: num("Отступ", g.get_s_padding(), st.padding).max(0.0),
            corner_radius: num("Скругление", g.get_s_corner(), st.corner_radius).max(0.0),
            letter_spacing: num("Межбуквенный интервал", g.get_s_spacing(), st.letter_spacing).clamp(-20.0, 100.0),
            uppercase: g.get_s_upper(),
        }
    };
    if let ElementKind::Image { path } = &e.kind {
        let next = g.get_p_path().trim().to_string();
        if *path != next
            && let Ok(natural) = image::image_dimensions(&next)
        {
            let (fw, fh) = frame(c);
            let region = placement::region_of(c.gfx.draft.as_ref().unwrap(), &e.id, fw, fh);
            let (w, h) = placement::fit_image_size(natural, placement::rect_in(&e, region));
            e.placement.width = Some(w);
            e.placement.height = Some(h);
            g.set_p_w(s(fmt_num(w)));
            g.set_p_h(s(fmt_num(h)));
        }
    }
    e.kind = match e.kind.clone() {
        ElementKind::Text { style, .. } => {
            ElementKind::Text { text: g.get_p_text().to_string(), style: style_from(&style, &g, &mut color, &mut num) }
        }
        ElementKind::Ticker { style, speed, width, .. } => ElementKind::Ticker {
            text: g.get_p_text().to_string(),
            speed: num("Скорость", g.get_p_speed(), speed).clamp(1.0, 2000.0),
            width: if g.get_p_twidth().trim().is_empty() {
                None
            } else {
                Some(num("Ширина полосы", g.get_p_twidth(), width.unwrap_or(0.0)).max(10.0))
            },
            style: style_from(&style, &g, &mut color, &mut num),
        },
        ElementKind::Clock { style, .. } => {
            ElementKind::Clock { format: g.get_p_format().to_string(), style: style_from(&style, &g, &mut color, &mut num) }
        }
        ElementKind::Countdown { style, exclude, .. } => ElementKind::Countdown {
            target: TARGETS.get(g.get_p_target().max(0) as usize).copied().unwrap_or_default(),
            prefix: g.get_p_prefix().to_string(),
            style: style_from(&style, &g, &mut color, &mut num),
            exclude,
            format: TimeFormat::ALL.get(g.get_p_tformat().max(0) as usize).copied().unwrap_or_default(),
        },
        ElementKind::Progress { color: old, width, height, radius, track, exclude, shape, .. } => {
            let mut extra = Vec::new();
            let shape = design::read_shape(&g, &shape, &mut num, &mut color, &mut extra);
            errors.borrow_mut().extend(extra);
            ElementKind::Progress {
                color: color("Цвет заполнения", g.get_p_color(), true).unwrap_or(old),
                gradient: color("Градиент заполнения", g.get_pg_gradient(), false),
                track: color("Цвет полосы", g.get_pg_track(), true).unwrap_or(track),
                width: num("Ширина", g.get_p_rw(), width).max(2.0),
                height: num("Высота", g.get_p_rh(), height).max(1.0),
                radius: num("Скругление", g.get_p_radius(), radius).max(0.0),
                target: if g.get_pg_target() == 1 { ProgressTarget::Item } else { ProgressTarget::Program },
                direction: FillDirection::ALL.get(g.get_pg_direction().max(0) as usize).copied().unwrap_or_default(),
                exclude,
                shape,
            }
        }
        ElementKind::Image { .. } => ElementKind::Image { path: g.get_p_path().trim().to_string() },
        ElementKind::Rect { color: old, width, height, radius, shape } => {
            let mut extra = Vec::new();
            let shape = design::read_shape(&g, &shape, &mut num, &mut color, &mut extra);
            errors.borrow_mut().extend(extra);
            ElementKind::Rect {
                color: color("Цвет фигуры", g.get_p_color(), true).unwrap_or(old),
                width: num("Ширина фигуры", g.get_p_rw(), width).max(1.0),
                height: num("Высота фигуры", g.get_p_rh(), height).max(1.0),
                radius: num("Скругление", g.get_p_radius(), radius).max(0.0),
                shape,
            }
        }
        kind @ ElementKind::Plate { .. } => {
            let mut extra = Vec::new();
            let mut style_num = |l: &str, v: SharedString, o: f32| -> f32 {
                match v.trim().replace(',', ".").parse::<f32>() {
                    Ok(x) if x.is_finite() => x,
                    _ => {
                        errors.borrow_mut().push(format!("{l}: нужно число"));
                        o
                    }
                }
            };
            let mut style_color = |l: &str, v: SharedString, required: bool| -> Option<String> {
                let t = v.trim().to_string();
                if t.is_empty() && !required {
                    return None;
                }
                if placement::valid_color(&t) {
                    Some(t)
                } else {
                    errors.borrow_mut().push(format!("{l}: цвет в формате #RRGGBB или #RRGGBBAA"));
                    None
                }
            };
            let style = |st: &TextStyle| style_from(st, &g, &mut style_color, &mut style_num);
            let k = design::read_plate(c, &g, kind, style, &mut num, &mut color, &mut extra);
            errors.borrow_mut().extend(extra);
            k
        }
        ElementKind::Group { .. } => {
            ElementKind::Group { children: g.get_p_children().split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect() }
        }
    };
    e.animation = Animation {
        anim_in: ANIMS.get(g.get_a_in().max(0) as usize).copied().unwrap_or_default(),
        anim_out: ANIMS.get(g.get_a_out().max(0) as usize).copied().unwrap_or_default(),
        duration: match g.get_a_ms().trim().parse::<i64>() {
            Ok(ms) if (0..=10_000).contains(&ms) => Dur(ms),
            _ => {
                errors.borrow_mut().push("Длительность анимации: 0–10000 мс".into());
                e.animation.duration
            }
        },
    };
    e.profiles = g.get_p_profiles().split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    g.set_prop_errors(s(errors.borrow().join(" · ")));
    swatches(c);
    mutate(c, Some("props"), move |cfg| cfg.elements[i] = e);
    update_item(c, i);
    flags(c);
}

/// Choose the image of the selected element with a file dialog.
fn pick_image(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let current = PathBuf::from(g.get_p_path().trim());
    let mut dlg = rfd::FileDialog::new()
        .set_title("Файл графики")
        .add_filter("Графика и анимации", &["png", "jpg", "jpeg", "webp", "html", "htm", "swf"]);
    if let Some(dir) = current.parent().filter(|d| d.is_absolute() && d.is_dir()) {
        dlg = dlg.set_directory(dir);
    }
    let Some(path) = dlg.pick_file() else { return };
    c.gfx.force_render = true;
    g.set_p_path(s(path.to_string_lossy().to_string()));
    props_changed(c);
}

// ------------------------------------------------------------ canvas edits

fn drag_start(c: &mut Ctx, i: usize, handle: i32) {
    let (fw, fh) = frame(c);
    let Some(cfg) = c.gfx.draft.clone() else { return };
    let Some(e) = cfg.elements.get(i) else { return };
    if c.gfx.locked.contains(&e.id) {
        return;
    }
    let region = placement::region_of(&cfg, &e.id, fw, fh);
    let start = placement::rect_in(e, region);
    c.gfx.selected = Some(i);
    c.gfx.drag = Some(Drag { index: i, handle, start, region, before: cfg });
    if let Some(ui) = c.window() {
        ui.global::<Gfx>().set_selected(i as i32);
    }
    props(c);
}

fn drag_move(c: &mut Ctx, i: usize, handle: i32, dx: f32, dy: f32) {
    let Some(d) = c.gfx.drag.as_ref() else { return };
    if d.index != i || d.handle != handle {
        return;
    }
    let (fw, fh) = frame(c);
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let snap = g.get_snap();
    let (mut r, mut vg, mut hg) = (Rect { x: d.start.x + dx, y: d.start.y + dy, ..d.start }, vec![], vec![]);
    if handle >= 0 {
        r = placement::resize(d.start, handle, dx, dy, 8.0);
    } else if snap {
        let s = placement::snap(r, fw, fh, 8.0);
        r = s.rect;
        vg = s.vguides;
        hg = s.hguides;
    }
    let resized = handle >= 0;
    let region = d.region;
    if let Some(e) = c.gfx.draft.as_mut().and_then(|d| d.elements.get_mut(i)) {
        if resized {
            e.placement.width = Some(r.w.round());
            e.placement.height = Some(r.h.round());
        }
        e.placement = placement::placement_in(e.placement.clone(), r, region);
    }
    g.set_vguides(vec_model(vg));
    g.set_hguides(vec_model(hg));
    update_item(c, i);
    if let Some(e) = c.gfx.draft.as_ref().and_then(|d| d.elements.get(i)).cloned() {
        pos_fields(c, &e);
    }
    // A ready selection plane follows the pointer locally. The exact server
    // snapshot is requested on release; no PNG roundtrip per mouse event.
    if !has_edit_layer(c) || c.gfx.pending_web {
        schedule_render(c);
    }
}

fn drag_end(c: &mut Ctx) {
    let Some(d) = c.gfx.drag.take() else { return };
    if let Some(after) = c.gfx.draft.clone() {
        c.gfx.history.record(&d.before, &after);
    }
    c.gfx.last_edit = None;
    if let Some(ui) = c.window() {
        let g = ui.global::<Gfx>();
        g.set_vguides(vec_model(Vec::new()));
        g.set_hguides(vec_model(Vec::new()));
    }
    flags(c);
    schedule_render(c);
}

fn nudge(c: &mut Ctx, dx: f32, dy: f32) {
    let Some(i) = c.gfx.selected else { return };
    let (fw, fh) = frame(c);
    mutate(c, Some("nudge"), move |cfg| {
        let Some(id) = cfg.elements.get(i).map(|e| e.id.clone()) else { return };
        let region = placement::region_of(cfg, &id, fw, fh);
        if let Some(e) = cfg.elements.get_mut(i) {
            let r = placement::rect_in(e, region);
            e.placement = placement::placement_in(e.placement.clone(), Rect { x: r.x + dx, y: r.y + dy, ..r }, region);
        }
    });
    update_item(c, i);
    if let Some(e) = c.gfx.draft.as_ref().and_then(|d| d.elements.get(i)).cloned() {
        pos_fields(c, &e);
    }
    flags(c);
}

/// Count / stop counting content kind `KINDS[i]` (programme countdown,
/// progress bar).
fn toggle_exclude(c: &mut Ctx, i: usize) {
    let Some(sel) = c.gfx.selected else { return };
    let Some(k) = KINDS.get(i).copied() else { return };
    mutate(c, None, move |cfg| {
        let Some(e) = cfg.elements.get_mut(sel) else { return };
        if let ElementKind::Countdown { exclude, .. } | ElementKind::Progress { exclude, .. } = &mut e.kind {
            match exclude.iter().position(|x| *x == k) {
                Some(p) => {
                    exclude.remove(p);
                }
                None => exclude.push(k),
            }
        }
    });
    update_item(c, sel);
    props(c);
    flags(c);
}

fn unique_id(cfg: &GraphicsConfig, base: &str) -> String {
    let mut n = 1;
    loop {
        let id = format!("{base}-{n}");
        if !cfg.elements.iter().any(|e| e.id == id) {
            return id;
        }
        n += 1;
    }
}

fn add(c: &mut Ctx, kind: &str) {
    let mut cfg = c.gfx.draft.clone().unwrap_or_default();
    let style = TextStyle { size: 40.0, background: Some("#00000099".into()), ..Default::default() };
    let (k, name, anchor, x, y) = match kind {
        "rect" => (
            ElementKind::Rect { color: "#1E3A8ACC".into(), width: 600.0, height: 90.0, radius: 12.0, shape: Default::default() },
            "Фигура",
            lb_core::graphics::Anchor::BottomLeft,
            96.0,
            120.0,
        ),
        "plate" => (design::new_plate(), "Плашка", lb_core::graphics::Anchor::BottomLeft, 96.0, 110.0),
        "image" => (ElementKind::Image { path: "logo.png".into() }, "Логотип", lb_core::graphics::Anchor::TopRight, 60.0, 50.0),
        "web" => (ElementKind::Image { path: String::new() }, "HTML / SWF", lb_core::graphics::Anchor::TopLeft, 0.0, 0.0),
        // the file the channel form stores (`channels/<id>/avatar.png`)
        "avatar" => (
            ElementKind::Image { path: format!("channels/{}/avatar.png", c.model.ch()) },
            "Аватар канала",
            lb_core::graphics::Anchor::TopRight,
            60.0,
            50.0,
        ),
        "clock" => {
            (ElementKind::Clock { format: "%H:%M".into(), style: style.clone() }, "Часы", lb_core::graphics::Anchor::TopRight, 60.0, 50.0)
        }
        "countdown" => (
            ElementKind::Countdown {
                target: CountdownTarget::NextProgram,
                style: style.clone(),
                prefix: "Через ".into(),
                exclude: vec![],
                format: TimeFormat::Auto,
            },
            "Таймер",
            lb_core::graphics::Anchor::TopLeft,
            60.0,
            50.0,
        ),
        "program-left" => (
            ElementKind::Countdown {
                target: CountdownTarget::ProgramRemaining,
                style: style.clone(),
                prefix: "До конца передачи ".into(),
                // ads and promos inside the programme do not count
                exclude: vec![lb_core::media::MediaKind::Ad, lb_core::media::MediaKind::Promo],
                format: TimeFormat::Auto,
            },
            "До конца передачи",
            lb_core::graphics::Anchor::TopLeft,
            60.0,
            110.0,
        ),
        "progress" => (
            ElementKind::Progress {
                width: 640.0,
                height: 12.0,
                color: "#E11D48".into(),
                gradient: Some("#F59E0B".into()),
                track: "#FFFFFF40".into(),
                radius: 6.0,
                target: ProgressTarget::Program,
                exclude: vec![lb_core::media::MediaKind::Ad, lb_core::media::MediaKind::Promo],
                direction: FillDirection::LeftToRight,
                shape: Default::default(),
            },
            "Прогресс передачи",
            lb_core::graphics::Anchor::BottomLeft,
            96.0,
            60.0,
        ),
        "ticker" => (
            ElementKind::Ticker { text: "{ticker}".into(), style: style.clone(), speed: 120.0, width: None },
            "Бегущая строка",
            lb_core::graphics::Anchor::BottomLeft,
            0.0,
            40.0,
        ),
        _ => (ElementKind::Text { text: "{title}".into(), style }, "Текст", lb_core::graphics::Anchor::BottomLeft, 96.0, 120.0),
    };
    // the layer of its kind (made when new), not a layer of its own
    let layer = layers::layer_for_new(c, &mut cfg, &k);
    let new_layers = cfg.layers.clone();
    let e = Element {
        id: unique_id(&cfg, kind),
        name: name.into(),
        kind: k,
        placement: Placement {
            anchor,
            x,
            y,
            width: match kind {
                "web" => Some(640.0),
                "image" => Some(320.0),
                "avatar" => Some(96.0),
                _ => None,
            },
            height: match kind {
                "web" => Some(360.0),
                "image" => Some(180.0),
                "avatar" => Some(96.0),
                _ => None,
            },
        },
        opacity: 1.0,
        layer,
        animation: Animation { anim_in: AnimKind::Fade, anim_out: AnimKind::Fade, duration: Dur(500) },
        profiles: vec![],
    };
    mutate(c, None, |cfg| {
        cfg.layers = new_layers;
        cfg.elements.push(e);
    });
    c.gfx.collapsed.remove(&layer);
    c.gfx.selected = c.gfx.draft.as_ref().map(|d| d.elements.len() - 1);
    refresh(c);
}

fn duplicate(c: &mut Ctx) {
    let Some(i) = c.gfx.selected else { return };
    let Some(cfg) = c.gfx.draft.clone() else { return };
    let Some(mut e) = cfg.elements.get(i).cloned() else { return };
    e.id = unique_id(&cfg, kind_id(&e.kind));
    e.name = format!("{} (копия)", label(&e));
    e.placement.x += 24.0;
    e.placement.y += 24.0;
    mutate(c, None, |cfg| cfg.elements.push(e));
    c.gfx.selected = c.gfx.draft.as_ref().map(|d| d.elements.len() - 1);
    refresh(c);
}

/// Delete the selected element, after a confirmation unless it was switched off.
fn remove(c: &mut Ctx) {
    let Some(e) = c.gfx.selected.and_then(|i| c.gfx.draft.as_ref()?.elements.get(i)).cloned() else { return };
    if !c.prefs.confirm_graphic_delete {
        return remove_now(c);
    }
    let id = e.id.clone();
    c.ask_with_option(
        "Удалить элемент?",
        &format!("«{}» будет удалён вместе с правилами показа. Удаление можно отменить.", label(&e)),
        "Удалить",
        "Больше не напоминать",
        move |c, never_again| {
            if never_again {
                c.prefs.confirm_graphic_delete = false;
                c.prefs.save();
            }
            // the selection may have changed while the dialog was open
            if let Some(i) = c.gfx.draft.as_ref().and_then(|d| d.elements.iter().position(|e| e.id == id)) {
                c.gfx.selected = Some(i);
                remove_now(c);
            }
        },
    );
}

fn remove_now(c: &mut Ctx) {
    let Some(i) = c.gfx.selected else { return };
    let Some(e) = c.gfx.draft.as_ref().and_then(|d| d.elements.get(i)).cloned() else { return };
    let rules = c.gfx.draft.as_ref().map(|d| d.rules.iter().filter(|r| r.element == e.id).count()).unwrap_or(0);
    // one edit, so a single undo brings the element back with its rules
    mutate(c, None, |cfg| {
        cfg.elements.remove(i);
        cfg.rules.retain(|r| r.element != e.id);
        for other in &mut cfg.elements {
            if let ElementKind::Group { children } = &mut other.kind {
                children.retain(|ch| *ch != e.id);
            }
        }
    });
    c.gfx.selected = None;
    refresh(c);
    c.toast_with(
        format!("Элемент «{}» удалён{}", label(&e), if rules > 0 { format!(" вместе с правилами показа: {rules}") } else { String::new() }),
        "Отменить",
        undo,
    );
}

fn toggle_flag(c: &mut Ctx, i: usize, hidden: bool) {
    let Some(id) = c.gfx.draft.as_ref().and_then(|d| d.elements.get(i)).map(|e| e.id.clone()) else { return };
    let set = if hidden { &mut c.gfx.hidden } else { &mut c.gfx.locked };
    if !set.remove(&id) {
        set.insert(id);
    }
    refresh(c);
}

fn live(c: &mut Ctx, action: &str) {
    let Some(id) = c.gfx.selected.and_then(|i| c.gfx.draft.as_ref().and_then(|d| d.elements.get(i))).map(|e| e.id.clone()) else { return };
    if c.gfx.scope.is_some() {
        c.toast("Графика сцены показывается сама, пока сцена в эфире — по своим правилам", false);
        return;
    }
    if c.gfx.saved.as_ref().is_some_and(|s| !s.elements.iter().any(|e| e.id == id)) {
        c.toast("Элемент ещё не сохранён — сохраните оформление, чтобы показать его в эфире", true);
        return;
    }
    let body = if action == "auto" {
        json!({ "cmd": "graphic_auto", "element": id })
    } else {
        json!({ "cmd": "graphic", "element": id, "action": action })
    };
    c.command(body);
}

// ------------------------------------------------------------ render / animation

fn invalidate_layers(c: &mut Ctx) {
    c.gfx.preview_layer = None;
    if let Some(ui) = c.window() {
        ui.global::<Gfx>().set_edit_layered(false);
    }
}

fn has_edit_layer(c: &Ctx) -> bool {
    c.gfx
        .preview_layer
        .as_ref()
        .is_some_and(|id| c.gfx.selected.and_then(|i| c.gfx.draft.as_ref()?.elements.get(i)).is_some_and(|e| &e.id == id))
}

fn selected_geometry(c: &Ctx) -> Option<(String, Rect)> {
    let cfg = c.gfx.draft.as_ref()?;
    let e = cfg.elements.get(c.gfx.selected?)?;
    let (w, h) = frame(c);
    Some((e.id.clone(), placement::rect_in(e, placement::region_of(cfg, &e.id, w, h))))
}

fn update_preview_target(c: &Ctx) {
    if has_edit_layer(c)
        && let Some((_, rect)) = selected_geometry(c)
        && let Some(ui) = c.window()
    {
        let g = ui.global::<Gfx>();
        g.set_edit_x(rect.x);
        g.set_edit_y(rect.y);
        g.set_edit_w(rect.w);
        g.set_edit_h(rect.h);
    }
}

fn schedule_render(c: &mut Ctx) {
    if c.gfx.render_scheduled != 0 {
        return;
    }
    static NEXT_TIMER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let ticket = NEXT_TIMER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    c.gfx.render_scheduled = ticket;
    // Leading throttle, not trailing debounce: continuous typing/nudging must
    // keep updating instead of waiting for the user to stop for 150 ms.
    slint::Timer::single_shot(Duration::from_millis(33), move || {
        with(move |c| {
            if c.gfx.render_scheduled != ticket {
                return;
            }
            c.gfx.render_scheduled = 0;
            if c.section() == "graphics" && !(c.gfx.drag.is_some() && has_edit_layer(c) && !c.gfx.pending_web) {
                render(c);
                plate_editor::preview(c);
            }
        })
    });
}

fn png_image(bytes: &[u8]) -> Option<crate::net::Frame> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(crate::net::Frame { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn to_image(f: &crate::net::Frame) -> Option<Image> {
    (f.rgba.len() == (f.width * f.height * 4) as usize)
        .then(|| Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height)))
}

/// Background of the canvas according to the mode.
pub fn render(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    match g.get_bg_mode() {
        1 => {
            live_background(c);
            return;
        }
        2 => {
            g.set_has_background(false);
            g.set_bg_note(SharedString::new());
            return;
        }
        _ => {}
    }
    if c.model.ch().is_empty() {
        return;
    }
    let Some(cfg) = c.gfx.draft.clone() else { return };
    if c.gfx.drag.is_some() && has_edit_layer(c) && !c.gfx.pending_web && !c.gfx.force_render {
        return;
    }
    if c.gfx.rendering {
        c.gfx.render_again = true;
        return;
    }
    c.gfx.rendering = true;
    static NEXT_RENDER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let ticket = NEXT_RENDER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    c.gfx.render_ticket = ticket;
    g.set_rendering(true);
    let ch = c.model.ch();
    let vars = cfg.vars.clone();
    let refresh = std::mem::take(&mut c.gfx.force_render);
    let width = if c.gfx.drag.is_some() { 640 } else { 1280 };
    let scope = c.gfx.scope.clone();
    let (fw, fh) = frame(c);
    let requested_geometry = selected_geometry(c);
    // Off-frame pixels are absent from the atlas. Request full renders while
    // moving such an element so returning it to the frame restores its content.
    let selected = requested_geometry.clone().filter(|(_, rect)| crate::logic::graphics_preview::can_transform_locally(*rect, fw, fh));
    let atlas_height = if selected.is_some() { (fh * (width as f32).min(fw) / fw).round().max(1.0) as u32 * 3 } else { 0 };
    c.worker.spawn(move |api| {
        let r = api.post_bytes_header(
            &format!("/channels/{ch}/graphics/render"),
            &json!({ "config": cfg, "vars": vars, "width": width,
            "interactive": true, "refresh": refresh, "edit_layer": selected.as_ref().map(|(id, _)| id) }),
            "x-lb-preview-pending",
        );
        let pending = r.as_ref().ok().is_some_and(|(_, header)| header.as_deref() == Some("1"));
        let frame = r.as_ref().ok().and_then(|(b, _)| edit_preview(b, atlas_height));
        let err = r.err();
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if c.gfx.render_ticket != ticket {
                return;
            }
            c.gfx.rendering = false;
            // Publish completed work even when another move is queued. Dropping
            // every intermediate frame starves the canvas until pointer release.
            let again = std::mem::take(&mut c.gfx.render_again) && c.section() == "graphics";
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Gfx>();
            g.set_rendering(false);
            if c.gfx.scope != scope
                || c.model.ch() != ch
                || requested_geometry.as_ref().map(|s| &s.0) != selected_geometry(c).as_ref().map(|s| &s.0)
            {
                if c.section() == "graphics" {
                    render(c);
                }
                return;
            }
            c.gfx.pending_web = pending;
            match frame.as_ref().and_then(|f| to_image(&f.full)) {
                Some(img) => {
                    g.set_background(img);
                    invalidate_layers(c);
                    if let (Some(preview), Some((id, rect))) = (&frame, &selected) {
                        if preview.planes.len() == 3 {
                            g.set_edit_planes(vec_model(preview.planes.iter().filter_map(to_image).collect()));
                            g.set_edit_base_x(rect.x);
                            g.set_edit_base_y(rect.y);
                            g.set_edit_base_w(rect.w);
                            g.set_edit_base_h(rect.h);
                            // text is drawn at its own size whatever box it is given
                            g.set_edit_stretch(!c.gfx.draft.as_ref().is_some_and(|d| {
                                d.elements.iter().any(|e| {
                                    &e.id == id
                                        && matches!(e.kind, ElementKind::Text { .. } | ElementKind::Clock { .. } | ElementKind::Countdown { .. })
                                })
                            }));
                            c.gfx.preview_layer = Some(id.clone());
                            update_preview_target(c);
                            g.set_edit_layered(true);
                        }
                    }
                    g.set_has_background(true);
                    g.set_bg_note(if pending {
                        "Загрузка первого кадра HTML / SWF…".into()
                    } else {
                        SharedString::new()
                    });
                }
                None => {
                    let error = err.unwrap_or_else(|| "пустой ответ".into());
                    if has_edit_layer(c) {
                        g.set_bg_note(s(format!("Не удалось обновить предпросмотр ({error}) — показана последняя готовая версия.")));
                    } else {
                        invalidate_layers(c);
                        g.set_bg_note(s(format!(
                            "Точный рендер недоступен ({error}) — показан кадр эфира, рамки элементов рассчитаны приблизительно."
                        )));
                        live_background(c);
                    }
                }
            }
            if again {
                render(c);
            } else if pending {
                slint::Timer::single_shot(Duration::from_millis(100), move || {
                    with(move |c| {
                        if c.gfx.render_ticket == ticket && c.section() == "graphics" {
                            render(c);
                        }
                    });
                });
            }
        })))
    });
}

fn live_background(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let app = ui.global::<App>();
    let g = ui.global::<Gfx>();
    g.set_has_background(app.get_has_preview());
    if app.get_has_preview() {
        g.set_background(app.get_preview());
    }
}

/// New preview frame arrived: refresh the live background.
pub fn preview_changed(c: &mut Ctx) {
    if c.section() == "graphics"
        && let Some(ui) = c.window()
        && ui.global::<Gfx>().get_bg_mode() == 1
    {
        live_background(c);
    }
}

/// Frames needed to show `dur_ms` at `fps` (at least 2, at most 150).
fn anim_steps(dur_ms: i64, fps: f64) -> i64 {
    ((dur_ms as f64 * fps / 1000.0).ceil() as i64).clamp(2, 150)
}

/// In-animation of the selected element as a short frame sequence.
fn preview_animation(c: &mut Ctx) {
    let Some(i) = c.gfx.selected else { return };
    let Some(cfg) = c.gfx.draft.clone() else { return };
    let Some(e) = cfg.elements.get(i).cloned() else { return };
    let dur = e.animation.duration.ms().max(100);
    let mut solo = cfg.clone();
    solo.elements = vec![e];
    solo.rules.clear();
    let ch = c.model.ch();
    // honours the preview frame rate setting
    let fps = crate::bridge::settings::preview::rate(c).clamp(1.0, 120.0);
    let steps = anim_steps(dur, fps);
    let interval = (1000.0 / fps).round().max(8.0) as i32;
    if let Some(ui) = c.window() {
        let g = ui.global::<Gfx>();
        g.set_rendering(true);
        g.set_anim_interval(interval);
    }
    c.worker.spawn(move |api| {
        let mut frames = Vec::new();
        let mut err = None;
        for k in 0..=steps {
            let t = dur * k / steps;
            match api.post_bytes(
                &format!("/channels/{ch}/graphics/render"),
                &json!({ "config": solo, "vars": solo.vars, "t_ms": t, "width": 960 }),
            ) {
                Ok(b) => frames.extend(png_image(&b)),
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Gfx>();
            g.set_rendering(false);
            if let Some(e) = err {
                c.toast(format!("Предпросмотр анимации требует рендера на сервере: {e}"), true);
                return;
            }
            let mut imgs: Vec<Image> = frames.iter().filter_map(to_image).collect();
            // hold the final frame for a moment before looping
            if let Some(last) = imgs.last().cloned() {
                imgs.extend(std::iter::repeat_n(last, (fps / 2.0).ceil() as usize));
            }
            g.set_anim_frames(vec_model(imgs));
            g.set_anim_index(0);
            g.set_anim_playing(true);
        })))
    });
}

// ------------------------------------------------------------ display rules

fn dur_txt(d: Option<Dur>) -> String {
    d.map(|d| d.to_string()).unwrap_or_default()
}

fn rules_rows(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let cfg = c.gfx.draft.clone().unwrap_or_default();
    let rows: Vec<GRuleRow> = cfg
        .rules
        .iter()
        .enumerate()
        .map(|(i, r)| GRuleRow {
            index: i as i32,
            id: s(r.id.clone()),
            name: s(if r.name.is_empty() { r.id.clone() } else { r.name.clone() }),
            enabled: r.enabled,
            element_index: cfg.elements.iter().position(|e| e.id == r.element).map(|i| i as i32).unwrap_or(-1),
            priority: s(r.priority.to_string()),
            after: s(dur_txt(r.timing.after_start)),
            duration: s(dur_txt(r.timing.duration)),
            before_end: s(dur_txt(r.timing.before_end)),
            last: s(dur_txt(r.timing.last)),
            every: s(dur_txt(r.timing.every)),
            at: s(r.timing.at.iter().map(|t| t.format("%H:%M").to_string()).collect::<Vec<_>>().join(", ")),
            categories: s(r.conditions.categories.join(", ")),
            xcategories: s(r.conditions.exclude_categories.join(", ")),
            media: s(r.conditions.media.join(", ")),
            xmedia: s(r.conditions.exclude_media.join(", ")),
            blocks: s(r.conditions.blocks.join(", ")),
            playlists: s(r.conditions.playlists.join(", ")),
            from: s(r.conditions.from.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()),
            to: s(r.conditions.to.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()),
            kinds: vec_model(KINDS.iter().map(|k| r.conditions.kinds.contains(k)).collect()),
            text_labels: vec_model(rule_texts(&cfg, r).into_iter().map(|t| t.label).collect()),
            texts: vec_model(rule_texts(&cfg, r)),
            hide_empty: r.hide_if_empty,
            error: s(if cfg.elements.iter().any(|e| e.id == r.element) { "" } else { "Элемент не найден" }),
        })
        .collect();
    ui.global::<Gfx>().set_rules(vec_model(rows));
}

/// Text fields of the rule's element (plate lines, text, ticker).
fn rule_texts(cfg: &GraphicsConfig, r: &GraphicRule) -> Vec<RuleTextRow> {
    let Some(el) = cfg.element(&r.element) else { return vec![] };
    let labels: Vec<String> = match &el.kind {
        ElementKind::Plate { fields, .. } => {
            fields.iter().enumerate().map(|(i, f)| if f.label.is_empty() { format!("Строка {}", i + 1) } else { f.label.clone() }).collect()
        }
        ElementKind::Ticker { .. } => vec!["Текст бегущей строки".into()],
        _ => vec!["Текст".into()],
    };
    lb_core::graphics::element_texts(el)
        .unwrap_or_default()
        .into_iter()
        .zip(labels)
        .enumerate()
        .map(|(k, (design, label))| RuleTextRow {
            label: s(label),
            design: s(design.to_string()),
            text: s(r.texts.get(k).cloned().unwrap_or_default()),
        })
        .collect()
}

fn rule_edited(c: &mut Ctx, i: usize, field: &str, v: &str) {
    let cfg = c.gfx.draft.clone().unwrap_or_default();
    let elements: Vec<String> = cfg.elements.iter().map(|e| e.id.clone()).collect();
    let dur = |v: &str| -> Option<Option<Dur>> { if v.trim().is_empty() { Some(None) } else { v.trim().parse().ok().map(Some) } };
    let v = v.to_string();
    let field = field.to_string();
    let key = format!("rule-{i}-{field}");
    let mut bad = false;
    let parsed_dur = dur(&v);
    let parsed_at: Option<Vec<chrono::NaiveTime>> =
        v.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).map(|x| crate::logic::timeline::parse_time(x).ok().flatten()).collect();
    match field.as_str() {
        "after" | "duration" | "before-end" | "last" | "every" => bad = parsed_dur.is_none(),
        "at" => bad = parsed_at.is_none(),
        "from" | "to" => bad = crate::logic::timeline::parse_time(&v).is_err(),
        "priority" => bad = v.trim().parse::<i32>().is_err(),
        _ => {}
    }
    if bad {
        c.toast("Неверное значение: время в формате ЧЧ:ММ:СС, приоритет — целое число", true);
        return;
    }
    mutate(c, Some(&key), move |cfg| {
        let Some(r) = cfg.rules.get_mut(i) else { return };
        match field.as_str() {
            "enabled" => r.enabled = v == "1",
            "element" => {
                if let Some(id) = v.parse::<usize>().ok().and_then(|k| elements.get(k)) {
                    r.element = id.clone();
                }
            }
            "priority" => r.priority = v.trim().parse().unwrap_or(r.priority),
            "after" => r.timing.after_start = parsed_dur.flatten(),
            "duration" => r.timing.duration = parsed_dur.flatten(),
            "before-end" => r.timing.before_end = parsed_dur.flatten(),
            "last" => r.timing.last = parsed_dur.flatten(),
            "every" => r.timing.every = parsed_dur.flatten(),
            "at" => r.timing.at = parsed_at.unwrap_or_default(),
            "name" => r.name = v.clone(),
            "media" => r.conditions.media = list(&v),
            "xmedia" => r.conditions.exclude_media = list(&v),
            "blocks" => r.conditions.blocks = list(&v),
            "playlists" => r.conditions.playlists = list(&v),
            "categories" => r.conditions.categories = list(&v),
            "xcategories" => r.conditions.exclude_categories = list(&v),
            "from" => r.conditions.from = crate::logic::timeline::parse_time(&v).ok().flatten(),
            "to" => r.conditions.to = crate::logic::timeline::parse_time(&v).ok().flatten(),
            "kind" => {
                if let Some(k) = v.parse::<usize>().ok().and_then(|k| KINDS.get(k)) {
                    match r.conditions.kinds.iter().position(|x| x == k) {
                        Some(p) => {
                            r.conditions.kinds.remove(p);
                        }
                        None => r.conditions.kinds.push(*k),
                    }
                }
            }
            "hide-empty" => r.hide_if_empty = v == "1",
            f => {
                if let Some(k) = f.strip_prefix("text:").and_then(|k| k.parse::<usize>().ok()) {
                    if r.texts.len() <= k {
                        r.texts.resize(k + 1, String::new());
                    }
                    r.texts[k] = v.clone();
                    while r.texts.last().is_some_and(|t| t.is_empty()) {
                        r.texts.pop();
                    }
                }
            }
        }
    });
    // typed text stays as typed (rebuilding the rows would reset the cursor)
    let typed = |k: &str| {
        ["-categories", "-xcategories", "-media", "-xmedia", "-blocks", "-playlists", "-name", "-at", "-from", "-to"]
            .iter()
            .any(|t| k.ends_with(t))
            || k.contains("-text:")
    };
    if matches!(c.gfx.last_edit.as_ref(), Some((k, _)) if !typed(k)) || field_is_structural(&key) {
        rules_rows(c);
    }
    flags(c);
}

/// Append `{var}` to a rule's own text of line `k` (an empty entry starts
/// from nothing: the design's text stays as the placeholder).
fn rule_insert_var(c: &mut Ctx, i: usize, k: usize, var: &str) {
    let var = var.to_string();
    mutate(c, None, move |cfg| {
        let Some(r) = cfg.rules.get_mut(i) else { return };
        if r.texts.len() <= k {
            r.texts.resize(k + 1, String::new());
        }
        r.texts[k] = append_var(&r.texts[k], &var);
    });
    rules_rows(c);
    flags(c);
}

fn field_is_structural(key: &str) -> bool {
    key.ends_with("-enabled") || key.ends_with("-element") || key.ends_with("-kind") || key.ends_with("-hide-empty")
}

/// "a, b" → ["a", "b"].
fn list(v: &str) -> Vec<String> {
    v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

fn rule_add(c: &mut Ctx) {
    let cfg = c.gfx.draft.clone().unwrap_or_default();
    let Some(el) = c.gfx.selected.and_then(|i| cfg.elements.get(i)).or(cfg.elements.first()).map(|e| e.id.clone()) else {
        c.toast("Сначала добавьте элемент графики", true);
        return;
    };
    let mut n = cfg.rules.len() + 1;
    while cfg.rules.iter().any(|r| r.id == format!("show-{n}")) {
        n += 1;
    }
    let rule: GraphicRule = match serde_json::from_value(json!({ "id": format!("show-{n}"), "name": format!("Показ {n}"), "element": el }))
    {
        Ok(r) => r,
        Err(e) => {
            c.toast(format!("Не удалось создать правило: {e}"), true);
            return;
        }
    };
    mutate(c, None, |cfg| cfg.rules.push(rule));
    refresh(c);
}

// ------------------------------------------------------------ variables

fn sorted_vars(c: &Ctx) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> =
        c.gfx.draft.as_ref().map(|d| d.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    v.sort();
    v
}

fn vars_rows(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    ui.global::<Gfx>().set_vars(vec_model(sorted_vars(c).into_iter().map(|(k, v)| VarRow { key: s(k), value: s(v) }).collect()));
}

fn var_edited(c: &mut Ctx, i: usize, value: String) {
    let Some(key) = sorted_vars(c).get(i).map(|(k, _)| k.clone()) else { return };
    let k2 = format!("var-{key}");
    mutate(c, Some(&k2), move |cfg| {
        cfg.vars.insert(key, value);
    });
    flags(c);
}

fn var_add(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let key = g.get_new_key().trim().trim_matches(|ch| ch == '{' || ch == '}').to_string();
    if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        c.toast("Имя переменной: латиница, цифры и «_»", true);
        return;
    }
    let value = g.get_new_value().to_string();
    mutate(c, None, |cfg| {
        cfg.vars.insert(key, value);
    });
    g.set_new_key(SharedString::new());
    g.set_new_value(SharedString::new());
    refresh(c);
}

// ------------------------------------------------------------ templates

fn templates_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("lauraboard").join("gfx-templates"))
}

fn templates_list(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let mut files: Vec<PathBuf> = templates_dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|it| it.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    ui.global::<Gfx>().set_templates(vec_model(
        files.iter().map(|p| s(p.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default())).collect(),
    ));
    c.gfx.templates = files;
}

fn template_save(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let name: String = ui
        .global::<Gfx>()
        .get_template_name()
        .trim()
        .chars()
        .map(|ch| if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == ' ' { ch } else { '_' })
        .collect();
    let (Some(dir), Some(cfg)) = (templates_dir(), c.gfx.draft.clone()) else { return };
    if name.is_empty() {
        return;
    }
    let path = dir.join(format!("{name}.json"));
    let result = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, serde_json::to_vec_pretty(&cfg).unwrap_or_default()));
    match result {
        Ok(()) => {
            c.toast(format!("Шаблон «{name}» сохранён"), false);
            ui.global::<Gfx>().set_template_name(SharedString::new());
        }
        Err(e) => c.toast(format!("Не удалось сохранить шаблон: {e}"), true),
    }
    templates_list(c);
}

fn template_apply(c: &mut Ctx, i: usize) {
    let Some(path) = c.gfx.templates.get(i).cloned() else { return };
    let parsed = std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice::<GraphicsConfig>(&b).map_err(|e| e.to_string()));
    match parsed {
        Ok(t) => {
            let name = path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            c.ask(
                "Применить шаблон?",
                &format!("Текущее оформление будет заменено шаблоном «{name}». Это можно отменить (Ctrl+Z) до сохранения."),
                "Применить",
                move |c| {
                    mutate(c, None, |cfg| *cfg = t);
                    c.gfx.selected = None;
                    refresh(c);
                },
            );
        }
        Err(e) => c.toast(format!("Шаблон повреждён: {e}"), true),
    }
}

fn template_delete(c: &mut Ctx, i: usize) {
    let Some(path) = c.gfx.templates.get(i).cloned() else { return };
    let name = path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    c.ask(
        "Удалить шаблон?",
        &format!("Шаблон «{name}» будет удалён с этого компьютера."),
        "Удалить",
        move |c| {
            if let Err(e) = std::fs::remove_file(&path) {
                c.toast(format!("Не удалось удалить: {e}"), true);
            }
            templates_list(c);
        },
    );
}

// ------------------------------------------------------------ undo / save

fn undo(c: &mut Ctx) {
    let Some(cur) = c.gfx.draft.clone() else { return };
    if let Some(prev) = c.gfx.history.undo(&cur) {
        c.gfx.draft = Some(prev);
        c.gfx.last_edit = None;
        refresh(c);
        schedule_render(c);
    }
}

fn redo(c: &mut Ctx) {
    let Some(cur) = c.gfx.draft.clone() else { return };
    if let Some(next) = c.gfx.history.redo(&cur) {
        c.gfx.draft = Some(next);
        c.gfx.last_edit = None;
        refresh(c);
        schedule_render(c);
    }
}

fn save(c: &mut Ctx) {
    if !c.gfx.dirty() {
        return;
    }
    if let Some(cfg) = c.gfx.draft.clone() {
        scope::save(c, cfg);
    }
}

pub fn shortcut(c: &mut Ctx, name: &str) {
    match name {
        "save" => save(c),
        "undo" => undo(c),
        "redo" => redo(c),
        "delete" => remove(c),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn append_var_spacing() {
        assert_eq!(super::append_var("", "{title}"), "{title}");
        assert_eq!(super::append_var("Далее:", "{title}"), "Далее: {title}");
        assert_eq!(super::append_var("Далее: ", "{title}"), "Далее: {title}");
    }
}

#[cfg(test)]
mod anim_tests {
    use super::anim_steps;

    #[test]
    fn frame_count_follows_rate() {
        assert_eq!(anim_steps(1000, 30.0), 30);
        assert_eq!(anim_steps(1000, 60.0), 60);
        assert_eq!(anim_steps(100, 10.0), 2);
        assert_eq!(anim_steps(60_000, 120.0), 150);
    }
}

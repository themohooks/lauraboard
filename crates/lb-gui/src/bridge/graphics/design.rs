//! Shape design fields (rectangles and plate backgrounds) and plate text
//! lines: showing them in the properties panel, reading them back, and
//! the line list actions (select, add, remove, reorder).

use slint::{ComponentHandle, SharedString};

use lb_core::graphics::{Accent, ElementKind, PlateField, ShapeStyle, Side, TextAlign, TextStyle};

use super::{color_of, fmt_num, mutate, props, s, update_item, with};
use crate::bridge::{Ctx, vec_model};
use crate::shell::{AppWindow, Gfx, PlateFieldRow};

const SIDES: [Side; 4] = [Side::Left, Side::Top, Side::Right, Side::Bottom];
const ALIGNS: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Gfx>();
    g.on_plate_field_select(|i| {
        with(move |c| {
            c.gfx.field = i.max(0) as usize;
            props(c);
        })
    });
    g.on_plate_field_add(|| with(add_field));
    g.on_plate_field_remove(|i| with(move |c| remove_field(c, i.max(0) as usize)));
    g.on_plate_field_move(|i, d| with(move |c| move_field(c, i.max(0) as usize, d)));
    g.on_pick_shape_image(|| with(pick_image));
}

/// Number readers / colour readers that collect errors (from `props_changed`).
pub type Num<'a> = dyn FnMut(&str, SharedString, f32) -> f32 + 'a;
pub type Col<'a> = dyn FnMut(&str, SharedString, bool) -> Option<String> + 'a;

pub fn show_shape(g: &Gfx, sh: &ShapeStyle) {
    g.set_sh_gradient(s(sh.gradient.clone().unwrap_or_default()));
    g.set_sh_angle(s(fmt_num(sh.gradient_angle)));
    g.set_sh_border(s(sh.border_color.clone().unwrap_or_default()));
    g.set_sh_border_w(s(fmt_num(sh.border_width)));
    g.set_sh_corners(s(sh.corners.map(|c| c.iter().map(|v| fmt_num(*v)).collect::<Vec<_>>().join(" ")).unwrap_or_default()));
    g.set_sh_skew(s(fmt_num(sh.skew)));
    g.set_sh_shadow(s(sh.shadow_color.clone().unwrap_or_default()));
    g.set_sh_shadow_blur(s(fmt_num(sh.shadow_blur)));
    g.set_sh_shadow_x(s(fmt_num(sh.shadow_offset.0)));
    g.set_sh_shadow_y(s(fmt_num(sh.shadow_offset.1)));
    match &sh.accent {
        Some(a) => {
            g.set_sh_accent_side(SIDES.iter().position(|x| *x == a.side).unwrap_or(0) as i32 + 1);
            g.set_sh_accent_size(s(fmt_num(a.size)));
            g.set_sh_accent_color(s(a.color.clone()));
        }
        None => {
            g.set_sh_accent_side(0);
            g.set_sh_accent_size(s("8"));
            g.set_sh_accent_color(s("#E11D48"));
        }
    }
    g.set_sh_image(s(sh.image.clone().unwrap_or_default()));
    swatches(g);
}

pub fn swatches(g: &Gfx) {
    g.set_sw_sh_gradient(color_of(&g.get_sh_gradient()));
    g.set_sw_sh_border(color_of(&g.get_sh_border()));
    g.set_sw_sh_shadow(color_of(&g.get_sh_shadow()));
    g.set_sw_sh_accent(color_of(&g.get_sh_accent_color()));
}

pub fn read_shape(g: &Gfx, old: &ShapeStyle, num: &mut Num, color: &mut Col, errors: &mut Vec<String>) -> ShapeStyle {
    let corners = {
        let t = g.get_sh_corners();
        let parts: Vec<f32> = t.split([' ', ',', ';']).filter(|p| !p.is_empty()).filter_map(|p| p.replace(',', ".").parse().ok()).collect();
        match parts.as_slice() {
            [] if t.trim().is_empty() => None,
            [a] => Some([*a; 4]),
            [a, b, c, d] => Some([*a, *b, *c, *d].map(|v| v.max(0.0))),
            _ => {
                errors.push("Углы: одно число или четыре через пробел".into());
                old.corners
            }
        }
    };
    let side = g.get_sh_accent_side();
    let accent = (side > 0).then(|| Accent {
        side: SIDES.get(side as usize - 1).copied().unwrap_or_default(),
        size: num("Ширина полосы", g.get_sh_accent_size(), old.accent.as_ref().map_or(8.0, |a| a.size)).max(1.0),
        color: color("Цвет полосы", g.get_sh_accent_color(), true).unwrap_or_else(|| "#E11D48".into()),
    });
    let image = g.get_sh_image().trim().to_string();
    ShapeStyle {
        gradient: color("Градиент", g.get_sh_gradient(), false),
        gradient_angle: num("Угол градиента", g.get_sh_angle(), old.gradient_angle),
        border_color: color("Рамка", g.get_sh_border(), false),
        border_width: num("Толщина рамки", g.get_sh_border_w(), old.border_width).max(0.0),
        corners,
        skew: num("Наклон", g.get_sh_skew(), old.skew).clamp(-45.0, 45.0),
        shadow_color: color("Тень", g.get_sh_shadow(), false),
        shadow_blur: num("Размытие тени", g.get_sh_shadow_blur(), old.shadow_blur).clamp(0.0, 100.0),
        shadow_offset: (num("Тень dx", g.get_sh_shadow_x(), old.shadow_offset.0), num("Тень dy", g.get_sh_shadow_y(), old.shadow_offset.1)),
        accent,
        image: (!image.is_empty()).then_some(image),
    }
}

/// Plate layout fields and its lines; returns the selected line's style
/// (shown in the text section).
pub fn show_plate(c: &Ctx, g: &Gfx, kind: &ElementKind) -> Option<TextStyle> {
    let ElementKind::Plate { fields, color, radius, shape, width, min_width, padding_x, padding_y, spacing, align } = kind else { return None };
    g.set_p_color(s(color.clone()));
    g.set_sw_p_color(color_of(color));
    g.set_p_radius(s(fmt_num(*radius)));
    g.set_pl_width(s(width.map(fmt_num).unwrap_or_default()));
    g.set_pl_min_width(s(fmt_num(*min_width)));
    g.set_pl_pad_x(s(fmt_num(*padding_x)));
    g.set_pl_pad_y(s(fmt_num(*padding_y)));
    g.set_pl_spacing(s(fmt_num(*spacing)));
    g.set_pl_align(ALIGNS.iter().position(|a| a == align).unwrap_or(0) as i32);
    let rows: Vec<PlateFieldRow> = fields.iter().map(|f| PlateFieldRow { label: s(f.label.clone()), text: s(f.text.clone()) }).collect();
    g.set_pl_fields(vec_model(rows));
    let i = c.gfx.field.min(fields.len().saturating_sub(1));
    g.set_pl_field(i as i32);
    show_shape(g, shape);
    let f = fields.get(i)?;
    g.set_pl_label(s(f.label.clone()));
    g.set_p_text(s(f.text.clone()));
    Some(f.style.clone())
}

/// Plate with its layout and the selected line read from the panel.
pub fn read_plate(c: &Ctx, g: &Gfx, old: ElementKind, style: impl FnOnce(&TextStyle) -> TextStyle, num: &mut Num, color: &mut Col, errors: &mut Vec<String>) -> ElementKind {
    let ElementKind::Plate { mut fields, color: old_color, radius, shape, width, min_width, padding_x, padding_y, spacing, align: _ } = old else {
        return old;
    };
    if let Some(f) = fields.get_mut(c.gfx.field) {
        f.label = g.get_pl_label().trim().to_string();
        f.text = g.get_p_text().to_string();
        f.style = style(&f.style);
    }
    let w = g.get_pl_width();
    ElementKind::Plate {
        fields,
        color: color("Цвет фона", g.get_p_color(), true).unwrap_or(old_color),
        radius: num("Скругление", g.get_p_radius(), radius).max(0.0),
        shape: read_shape(g, &shape, num, color, errors),
        width: if w.trim().is_empty() { None } else { Some(num("Ширина", w, width.unwrap_or(400.0)).max(10.0)) },
        min_width: num("Мин. ширина", g.get_pl_min_width(), min_width).max(0.0),
        padding_x: num("Поля ↔", g.get_pl_pad_x(), padding_x).max(0.0),
        padding_y: num("Поля ↕", g.get_pl_pad_y(), padding_y).max(0.0),
        spacing: num("Между строк", g.get_pl_spacing(), spacing),
        align: ALIGNS.get(g.get_pl_align().max(0) as usize).copied().unwrap_or_default(),
    }
}

/// A new plate: name and role lines over a dark background with an accent.
pub fn new_plate() -> ElementKind {
    let mut name = PlateField::new("Имя", "{presenter}", 40.0, "#FFFFFF");
    name.style.bold = true;
    ElementKind::Plate {
        fields: vec![name, PlateField::new("Подпись", "{title}", 26.0, "#FFD166")],
        color: "#101828E6".into(),
        radius: 6.0,
        shape: ShapeStyle {
            gradient: Some("#1E3A8AE6".into()),
            accent: Some(Accent { side: Side::Left, size: 10.0, color: "#E11D48".into() }),
            shadow_color: Some("#00000080".into()),
            shadow_blur: 12.0,
            shadow_offset: (0.0, 4.0),
            ..Default::default()
        },
        width: None,
        min_width: 420.0,
        padding_x: 28.0,
        padding_y: 14.0,
        spacing: 2.0,
        align: TextAlign::Left,
    }
}

/// Edit the selected plate's lines.
fn edit_fields(c: &mut Ctx, f: impl FnOnce(&mut Vec<PlateField>, &mut usize)) {
    let Some(i) = c.gfx.selected else { return };
    let mut sel = c.gfx.field;
    mutate(c, None, |cfg| {
        if let Some(ElementKind::Plate { fields, .. }) = cfg.elements.get_mut(i).map(|e| &mut e.kind) {
            f(fields, &mut sel);
        }
    });
    c.gfx.field = sel;
    update_item(c, i);
    props(c);
}

fn add_field(c: &mut Ctx) {
    edit_fields(c, |fields, sel| {
        let style = fields.last().map(|f| TextStyle { size: (f.style.size * 0.8).round().max(12.0), ..f.style.clone() }).unwrap_or_default();
        fields.push(PlateField { label: format!("Строка {}", fields.len() + 1), text: String::new(), style });
        *sel = fields.len() - 1;
    });
}

fn remove_field(c: &mut Ctx, i: usize) {
    edit_fields(c, |fields, sel| {
        if fields.len() > 1 && i < fields.len() {
            fields.remove(i);
            *sel = (*sel).min(fields.len() - 1);
        }
    });
}

fn move_field(c: &mut Ctx, i: usize, d: i32) {
    edit_fields(c, |fields, sel| {
        let j = i as i64 + d as i64;
        if j >= 0 && (j as usize) < fields.len() && i < fields.len() {
            fields.swap(i, j as usize);
            *sel = j as usize;
        }
    });
}

/// Texture picture for the selected shape.
fn pick_image(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let Some(path) = rfd::FileDialog::new().set_title("Картинка-фон").add_filter("Изображения", &["png", "jpg", "jpeg", "webp"]).pick_file() else { return };
    g.set_sh_image(s(path.to_string_lossy().to_string()));
    super::props_changed(c);
}

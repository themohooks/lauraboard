//! Advanced plate editor: an exact preview of the selected plate (a server
//! render of that element alone, cropped to what it draws) and ready-made
//! styles that redesign the plate while keeping its lines' text.

use serde_json::json;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

use lb_core::graphics::{Accent, ElementKind, PlateField, PlateLayout, ShapeStyle, Side, TextAlign, TextStyle, VAlign};

use super::{mutate, props, refresh, s, update_item, with};
use crate::bridge::{Ctx, vec_model};
use crate::net::Msg;
use crate::shell::{AppWindow, Gfx};

/// Margin around the plate in the preview, pixels of the frame.
const MARGIN: u32 = 24;

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Gfx>();
    g.set_pe_presets(vec_model(PRESETS.iter().map(|(n, _)| s(*n)).collect()));
    g.on_open_plate_editor(|| {
        with(|c| {
            if let Some(ui) = c.window() {
                ui.global::<Gfx>().set_pe_open(true);
            }
            preview(c);
        })
    });
    g.on_plate_preset(|i| with(move |c| apply_preset(c, i.max(0) as usize)));
    g.on_pick_plate_icon(|| with(pick_icon));
}

/// The selected element, when it is a plate.
fn selected_plate(c: &Ctx) -> Option<(usize, String)> {
    let i = c.gfx.selected?;
    let e = c.gfx.draft.as_ref()?.elements.get(i)?;
    matches!(e.kind, ElementKind::Plate { .. }).then(|| (i, e.id.clone()))
}

/// Render the selected plate alone (when the editor is open).
pub fn preview(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    if !g.get_pe_open() {
        return;
    }
    let Some((_, id)) = selected_plate(c) else {
        g.set_pe_open(false);
        return;
    };
    let Some(cfg) = c.gfx.draft.clone() else { return };
    let ch = c.model.ch();
    if ch.is_empty() {
        return;
    }
    g.set_pe_busy(true);
    let vars = cfg.vars.clone();
    c.worker.spawn(move |api| {
        let r = api.post_bytes(&format!("/channels/{ch}/graphics/render"), &json!({ "config": cfg, "vars": vars, "elements": [id] }));
        let cropped = r.ok().and_then(|b| crop_to_content(&b));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Gfx>();
            g.set_pe_busy(false);
            match cropped {
                Some((w, h, rgba)) => {
                    g.set_pe_preview(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h)));
                    g.set_pe_has_preview(true);
                }
                None => g.set_pe_has_preview(false),
            }
        })))
    });
}

/// PNG → the box of its visible pixels plus a margin (`None`: nothing drawn).
fn crop_to_content(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 {
        return None;
    }
    let (x0, y0) = (x0.saturating_sub(MARGIN), y0.saturating_sub(MARGIN));
    let (x1, y1) = ((x1 + MARGIN).min(w - 1), (y1 + MARGIN).min(h - 1));
    let sub = image::imageops::crop_imm(&img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
    Some((sub.width(), sub.height(), sub.into_raw()))
}

fn pick_icon(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let Some(path) = rfd::FileDialog::new().set_title("Картинка в плашке").add_filter("Изображения", &["png", "jpg", "jpeg", "webp"]).pick_file() else { return };
    g.set_pl_icon(s(path.to_string_lossy().to_string()));
    super::props_changed(c);
    props(c);
}

// ------------------------------------------------------------ ready-made styles

type Preset = fn(&mut PlateDesign);

/// What a style sets on a plate (its lines keep their text).
struct PlateDesign<'a> {
    fields: &'a mut Vec<PlateField>,
    color: &'a mut String,
    radius: &'a mut f32,
    shape: &'a mut ShapeStyle,
    min_width: &'a mut f32,
    padding_x: &'a mut f32,
    padding_y: &'a mut f32,
    spacing: &'a mut f32,
    align: &'a mut TextAlign,
    layout: &'a mut PlateLayout,
    valign: &'a mut VAlign,
}

impl PlateDesign<'_> {
    /// Text styles of the first and the other lines.
    fn lines(&mut self, first: TextStyle, rest: TextStyle) {
        for (i, f) in self.fields.iter_mut().enumerate() {
            let font = f.style.font.clone();
            f.style = if i == 0 { first.clone() } else { rest.clone() };
            // the chosen font stays: a style is about the look, not the typeface
            f.style.font = font;
            f.align = None;
            f.gap_before = 0.0;
        }
    }

    fn base(&mut self, color: &str, radius: f32, shape: ShapeStyle) {
        *self.color = color.into();
        *self.radius = radius;
        *self.shape = shape;
        *self.layout = PlateLayout::Column;
        *self.valign = VAlign::Center;
        *self.align = TextAlign::Left;
        *self.padding_x = 28.0;
        *self.padding_y = 14.0;
        *self.spacing = 2.0;
        *self.min_width = 420.0;
    }
}

fn text(size: f32, color: &str) -> TextStyle {
    TextStyle { size, color: color.into(), shadow_color: None, padding: 0.0, ..Default::default() }
}

fn bold(size: f32, color: &str) -> TextStyle {
    TextStyle { bold: true, ..text(size, color) }
}

fn shadow(hex: &str, blur: f32, dy: f32) -> ShapeStyle {
    ShapeStyle { shadow_color: Some(hex.into()), shadow_blur: blur, shadow_offset: (0.0, dy), ..Default::default() }
}

const PRESETS: [(&str, Preset); 8] = [
    ("Новости", |d| {
        d.base("#101828E6", 6.0, ShapeStyle { gradient: Some("#1E3A8AE6".into()), accent: Some(Accent { side: Side::Left, size: 10.0, color: "#E11D48".into() }), ..shadow("#00000080", 12.0, 4.0) });
        d.lines(bold(40.0, "#FFFFFF"), text(26.0, "#FFD166"));
    }),
    ("Срочно", |d| {
        d.base("#C8102E", 0.0, ShapeStyle { gradient: Some("#8B0000".into()), gradient_angle: 90.0, ..shadow("#00000099", 10.0, 4.0) });
        d.lines(TextStyle { uppercase: true, letter_spacing: 2.0, ..bold(42.0, "#FFFFFF") }, text(26.0, "#FFE4E6"));
    }),
    ("Спорт", |d| {
        d.base("#0F7B3F", 4.0, ShapeStyle { gradient: Some("#064E2B".into()), skew: -12.0, accent: Some(Accent { side: Side::Bottom, size: 6.0, color: "#FACC15".into() }), ..shadow("#00000080", 10.0, 4.0) });
        d.lines(TextStyle { uppercase: true, italic: true, ..bold(40.0, "#FFFFFF") }, TextStyle { italic: true, ..text(26.0, "#FACC15") });
    }),
    ("Минимал", |d| {
        d.base("#00000000", 0.0, ShapeStyle::default());
        *d.padding_x = 0.0;
        *d.padding_y = 0.0;
        *d.min_width = 0.0;
        let sh = |t: TextStyle| TextStyle { shadow_color: Some("#000000B0".into()), shadow_offset: (2.0, 2.0), ..t };
        d.lines(sh(bold(44.0, "#FFFFFF")), sh(text(28.0, "#E5E7EB")));
    }),
    ("Стекло", |d| {
        d.base("#FFFFFF26", 18.0, ShapeStyle { border_color: Some("#FFFFFF66".into()), border_width: 1.5, ..shadow("#00000060", 18.0, 6.0) });
        d.lines(bold(40.0, "#FFFFFF"), text(26.0, "#F1F5F9"));
    }),
    ("Две ленты", |d| {
        d.base("#00000000", 0.0, ShapeStyle::default());
        *d.padding_x = 0.0;
        *d.padding_y = 0.0;
        *d.spacing = 0.0;
        *d.min_width = 0.0;
        let band = |t: TextStyle, bg: &str, pad: f32| TextStyle { background: Some(bg.into()), padding: pad, ..t };
        d.lines(band(bold(40.0, "#FFFFFF"), "#E11D48", 12.0), band(text(26.0, "#FFFFFF"), "#101828E6", 10.0));
    }),
    ("Прямой эфир", |d| {
        d.base("#101828E6", 8.0, shadow("#00000080", 10.0, 4.0));
        *d.layout = PlateLayout::Row;
        *d.spacing = 14.0;
        *d.padding_x = 14.0;
        *d.padding_y = 10.0;
        *d.min_width = 0.0;
        d.lines(
            TextStyle { uppercase: true, letter_spacing: 1.5, background: Some("#E11D48".into()), padding: 8.0, corner_radius: 4.0, ..bold(26.0, "#FFFFFF") },
            text(30.0, "#FFFFFF"),
        );
    }),
    ("По центру", |d| {
        d.base("#0B1020D9", 30.0, ShapeStyle { border_color: Some("#38BDF8".into()), border_width: 2.0, ..shadow("#00000080", 14.0, 4.0) });
        *d.align = TextAlign::Center;
        *d.padding_x = 40.0;
        d.lines(bold(40.0, "#FFFFFF"), text(26.0, "#7DD3FC"));
    }),
];

fn apply_preset(c: &mut Ctx, i: usize) {
    let Some((sel, _)) = selected_plate(c) else { return };
    let Some((name, f)) = PRESETS.get(i) else { return };
    mutate(c, None, |cfg| {
        let Some(e) = cfg.elements.get_mut(sel) else { return };
        if let ElementKind::Plate { fields, color, radius, shape, min_width, padding_x, padding_y, spacing, align, layout, valign, .. } = &mut e.kind {
            f(&mut PlateDesign { fields, color, radius, shape, min_width, padding_x, padding_y, spacing, align, layout, valign });
        }
    });
    update_item(c, sel);
    refresh(c);
    c.toast(format!("Стиль «{name}» применён — текст строк сохранён"), false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_keep_the_text() {
        for (name, f) in PRESETS {
            let mut k = super::super::design::new_plate();
            let ElementKind::Plate { fields, color, radius, shape, min_width, padding_x, padding_y, spacing, align, layout, valign, .. } = &mut k else { unreachable!() };
            fields[0].text = "Иван".into();
            f(&mut PlateDesign { fields, color, radius, shape, min_width, padding_x, padding_y, spacing, align, layout, valign });
            assert_eq!(fields[0].text, "Иван", "{name}");
            assert_eq!(fields[1].text, "{title}", "{name}");
        }
    }

    #[test]
    fn crop_finds_the_drawn_box() {
        let mut img = image::RgbaImage::new(200, 100);
        for x in 50..70 {
            for y in 40..50 {
                img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (w, h, _) = crop_to_content(&png).unwrap();
        assert_eq!((w, h), (20 + 2 * MARGIN, 10 + 2 * MARGIN).min((200, 100)));
        let empty = image::RgbaImage::new(10, 10);
        let mut png = Vec::new();
        empty.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert!(crop_to_content(&png).is_none());
    }
}

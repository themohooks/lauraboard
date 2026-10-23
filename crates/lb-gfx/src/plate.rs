//! Plates (lower thirds): a designed background that grows to fit its
//! lines of text. Empty lines (a variable with no value) are left out.

use tiny_skia::{Pixmap, PixmapPaint, Transform};

use lb_core::graphics::{PlateField, ShapeStyle, Side, TextAlign};

use crate::fonts::FontStore;
use crate::images::ImageCache;
use crate::shape::render_shape;
use crate::text::render_text;

/// Layout of a plate apart from its fields.
pub struct PlateLook<'a> {
    pub color: &'a str,
    pub radius: f32,
    pub shape: &'a ShapeStyle,
    /// Fixed width (`None` = fit the text).
    pub width: Option<f32>,
    pub min_width: f32,
    pub min_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub spacing: f32,
    pub align: TextAlign,
}

/// A rendered plate: pixmap plus where the plate box starts in it.
pub struct PlateSprite {
    pub pixmap: Pixmap,
    pub ox: f32,
    pub oy: f32,
    pub w: f32,
    pub h: f32,
}

/// Render the plate with field texts already filled in (`texts[i]` is the
/// text of `fields[i]`).
pub fn render_plate(look: &PlateLook, fields: &[PlateField], texts: &[String], fonts: &mut FontStore, images: &mut ImageCache) -> Option<PlateSprite> {
    let mut lines: Vec<Pixmap> = Vec::new();
    for (f, t) in fields.iter().zip(texts) {
        if t.trim().is_empty() {
            continue;
        }
        let Some(font) = fonts.get_styled(f.style.font.as_deref(), f.style.bold, f.style.italic) else { continue };
        if let Some(pm) = render_text(&font, &f.style, t) {
            lines.push(pm);
        }
    }
    // the accent bar takes room from the text on its side
    let (mut pl, mut pr, mut pt, mut pb) = (look.padding_x, look.padding_x, look.padding_y, look.padding_y);
    if let Some(a) = &look.shape.accent {
        match a.side {
            Side::Left => pl += a.size,
            Side::Right => pr += a.size,
            Side::Top => pt += a.size,
            Side::Bottom => pb += a.size,
        }
    }
    let content_w = lines.iter().map(|l| l.width() as f32).fold(0.0, f32::max);
    let content_h = lines.iter().map(|l| l.height() as f32).sum::<f32>() + look.spacing * lines.len().saturating_sub(1) as f32;
    let w = look.width.unwrap_or(content_w + pl + pr).max(look.min_width).max(1.0);
    let h = (content_h + pt + pb).max(look.min_height).max(1.0);
    let bg = render_shape(look.color, w, h, look.radius, look.shape, images)?;
    let mut pm = bg.pixmap;
    // centre the text block vertically when the plate is taller than it
    let mut y = bg.oy + pt + ((h - pt - pb - content_h) / 2.0).max(0.0);
    let inner = w - pl - pr;
    for l in &lines {
        let lw = l.width() as f32;
        let x = bg.ox
            + pl
            + match look.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (inner - lw) / 2.0,
                TextAlign::Right => inner - lw,
            };
        pm.draw_pixmap(x.round() as i32, y.round() as i32, l.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        y += l.height() as f32 + look.spacing;
    }
    Some(PlateSprite { pixmap: pm, ox: bg.ox, oy: bg.oy, w, h })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn look(shape: &ShapeStyle) -> PlateLook<'_> {
        PlateLook {
            color: "#102030",
            radius: 0.0,
            shape,
            width: None,
            min_width: 0.0,
            min_height: 0.0,
            padding_x: 20.0,
            padding_y: 10.0,
            spacing: 4.0,
            align: TextAlign::Left,
        }
    }

    #[test]
    fn grows_with_text_and_skips_empty_lines() {
        let mut fonts = FontStore::new(Path::new("."));
        if fonts.get(None).is_none() {
            return; // no system fonts on this machine
        }
        let mut images = ImageCache::new(Path::new("."));
        let shape = ShapeStyle::default();
        let fields = vec![PlateField::new("Имя", "", 40.0, "#FFFFFF"), PlateField::new("Подпись", "", 24.0, "#FFD166")];
        let one = render_plate(&look(&shape), &fields, &["Иван Петров".into(), "".into()], &mut fonts, &mut images).unwrap();
        let two = render_plate(&look(&shape), &fields, &["Иван Петров".into(), "ведущий".into()], &mut fonts, &mut images).unwrap();
        assert!(two.h > one.h, "second line adds height");
        let long = render_plate(&look(&shape), &fields, &["Иван Петрович Сидоров-Петров".into(), "".into()], &mut fonts, &mut images).unwrap();
        assert!(long.w > one.w, "width follows the text");
        let mut fixed = look(&shape);
        fixed.width = Some(800.0);
        let f = render_plate(&fixed, &fields, &["x".into(), "".into()], &mut fonts, &mut images).unwrap();
        assert_eq!(f.w, 800.0);
    }
}

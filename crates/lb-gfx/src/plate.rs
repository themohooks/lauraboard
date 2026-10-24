//! Plates (lower thirds): a designed background that grows to fit its
//! lines of text. Empty lines (a variable with no value) are left out.
//! Lines go under each other or in a row, each with its own alignment and
//! gap; a fixed-width plate can shrink long lines to fit; a picture (logo,
//! photo) can sit beside the text.

use tiny_skia::{FillRule, Mask, Pixmap, PixmapPaint, Transform};

use lb_core::graphics::{PlateField, PlateIcon, PlateLayout, ShapeStyle, Side, TextAlign, VAlign};

use crate::fonts::FontStore;
use crate::images::ImageCache;
use crate::layout::rounded_rect;
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
    pub layout: PlateLayout,
    pub valign: VAlign,
    pub fit: bool,
    pub icon: Option<&'a PlateIcon>,
}

/// A rendered plate: pixmap plus where the plate box starts in it.
pub struct PlateSprite {
    pub pixmap: Pixmap,
    pub ox: f32,
    pub oy: f32,
    pub w: f32,
    pub h: f32,
}

/// A rendered line with its own alignment and gap before it.
struct Line {
    pm: Pixmap,
    align: TextAlign,
    gap: f32,
}

/// The picture of the plate at its height, corners rounded.
fn icon_pixmap(icon: &PlateIcon, images: &mut ImageCache) -> Option<Pixmap> {
    let orig = images.original(&icon.path)?;
    let h = icon.size.max(1.0);
    let w = (orig.width() as f32 * h / orig.height().max(1) as f32).max(1.0);
    let scaled = images.scaled(&icon.path, w.round() as u32, h.round() as u32)?;
    let mut pm = (*scaled).clone();
    if icon.radius > 0.0 {
        let mut mask = Mask::new(pm.width(), pm.height())?;
        let path = rounded_rect(0.0, 0.0, pm.width() as f32, pm.height() as f32, icon.radius)?;
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        let mut out = Pixmap::new(pm.width(), pm.height())?;
        out.draw_pixmap(0, 0, pm.as_ref(), &PixmapPaint::default(), Transform::identity(), Some(&mask));
        pm = out;
    }
    Some(pm)
}

/// Render the plate with field texts already filled in (`texts[i]` is the
/// text of `fields[i]`).
pub fn render_plate(look: &PlateLook, fields: &[PlateField], texts: &[String], fonts: &mut FontStore, images: &mut ImageCache) -> Option<PlateSprite> {
    let icon = look.icon.and_then(|i| icon_pixmap(i, images).map(|pm| (i, pm)));
    let icon_room = icon.as_ref().map_or(0.0, |(i, pm)| pm.width() as f32 + i.gap.max(0.0));
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
    // room for text when the width is fixed (long lines shrink into it)
    let text_room = look.width.map(|w| (w - pl - pr - icon_room).max(1.0));
    let mut lines: Vec<Line> = Vec::new();
    let present: Vec<(&PlateField, &String)> = fields.iter().zip(texts).filter(|(_, t)| !t.trim().is_empty()).collect();
    for (f, t) in &present {
        let Some(font) = fonts.get_styled(f.style.font.as_deref(), f.style.bold, f.style.italic) else { continue };
        let Some(mut pm) = render_text(&font, &f.style, t) else { continue };
        // in a row the lines share the room, so only a column shrinks per line
        if look.fit
            && look.layout == PlateLayout::Column
            && let Some(room) = text_room
            && pm.width() as f32 > room
        {
            let mut st = f.style.clone();
            st.size = (st.size * room / pm.width() as f32).floor().max(6.0);
            if let Some(p) = render_text(&font, &st, t) {
                pm = p;
            }
        }
        lines.push(Line { pm, align: f.align.unwrap_or(look.align), gap: f.gap_before.max(0.0) });
    }
    let n = lines.len();
    let (content_w, content_h) = match look.layout {
        PlateLayout::Column => (
            lines.iter().map(|l| l.pm.width() as f32).fold(0.0, f32::max),
            lines.iter().enumerate().map(|(i, l)| l.pm.height() as f32 + if i > 0 { l.gap } else { 0.0 }).sum::<f32>() + look.spacing * n.saturating_sub(1) as f32,
        ),
        PlateLayout::Row => (
            lines.iter().enumerate().map(|(i, l)| l.pm.width() as f32 + if i > 0 { l.gap } else { 0.0 }).sum::<f32>() + look.spacing * n.saturating_sub(1) as f32,
            lines.iter().map(|l| l.pm.height() as f32).fold(0.0, f32::max),
        ),
    };
    let icon_h = icon.as_ref().map_or(0.0, |(_, pm)| pm.height() as f32);
    let w = look.width.unwrap_or(content_w + icon_room + pl + pr).max(look.min_width).max(1.0);
    let h = (content_h.max(icon_h) + pt + pb).max(look.min_height).max(1.0);
    let bg = render_shape(look.color, w, h, look.radius, look.shape, images)?;
    let mut pm = bg.pixmap;
    let inner_h = h - pt - pb;
    let top_for = |hh: f32| {
        bg.oy
            + pt
            + match look.valign {
                VAlign::Top => 0.0,
                VAlign::Center => ((inner_h - hh) / 2.0).max(0.0),
                VAlign::Bottom => (inner_h - hh).max(0.0),
            }
    };
    // the picture, beside the text
    let icon_left = icon.as_ref().is_some_and(|(i, _)| i.side != Side::Right);
    if let Some((_, ipm)) = &icon {
        let x = if icon_left { bg.ox + pl } else { bg.ox + w - pr - ipm.width() as f32 };
        let y = top_for(ipm.height() as f32);
        pm.draw_pixmap(x.round() as i32, y.round() as i32, ipm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
    let text_x = bg.ox + pl + if icon_left { icon_room } else { 0.0 };
    let inner_w = w - pl - pr - icon_room;
    let aligned = |align: TextAlign, lw: f32| match align {
        TextAlign::Left => 0.0,
        TextAlign::Center => (inner_w - lw) / 2.0,
        TextAlign::Right => inner_w - lw,
    };
    match look.layout {
        PlateLayout::Column => {
            let mut y = top_for(content_h);
            for (i, l) in lines.iter().enumerate() {
                if i > 0 {
                    y += l.gap;
                }
                let x = text_x + aligned(l.align, l.pm.width() as f32);
                pm.draw_pixmap(x.round() as i32, y.round() as i32, l.pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                y += l.pm.height() as f32 + look.spacing;
            }
        }
        PlateLayout::Row => {
            // the row as a whole follows the plate's alignment
            let mut x = text_x + aligned(look.align, content_w);
            let top = top_for(content_h);
            for (i, l) in lines.iter().enumerate() {
                if i > 0 {
                    x += l.gap;
                }
                let y = top + (content_h - l.pm.height() as f32) / 2.0;
                pm.draw_pixmap(x.round() as i32, y.round() as i32, l.pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                x += l.pm.width() as f32 + look.spacing;
            }
        }
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
            layout: PlateLayout::Column,
            valign: VAlign::Center,
            fit: false,
            icon: None,
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

    #[test]
    fn row_layout_is_wide_and_low_and_fit_shrinks() {
        let mut fonts = FontStore::new(Path::new("."));
        if fonts.get(None).is_none() {
            return;
        }
        let mut images = ImageCache::new(Path::new("."));
        let shape = ShapeStyle::default();
        let fields = vec![PlateField::new("Метка", "", 30.0, "#FFFFFF"), PlateField::new("Текст", "", 30.0, "#FFFFFF")];
        let texts = ["ПРЯМОЙ ЭФИР".to_string(), "Новости дня".to_string()];
        let col = render_plate(&look(&shape), &fields, &texts, &mut fonts, &mut images).unwrap();
        let mut row = look(&shape);
        row.layout = PlateLayout::Row;
        let r = render_plate(&row, &fields, &texts, &mut fonts, &mut images).unwrap();
        assert!(r.w > col.w && r.h < col.h, "row {}x{} column {}x{}", r.w, r.h, col.w, col.h);
        // a long line in a narrow fixed plate: shrunk, so the plate keeps one line's height
        let mut narrow = look(&shape);
        narrow.width = Some(200.0);
        narrow.fit = true;
        let long = ["Очень длинное имя гостя студии".to_string(), String::new()];
        let shrunk = render_plate(&narrow, &fields, &long, &mut fonts, &mut images).unwrap();
        narrow.fit = false;
        let plain = render_plate(&narrow, &fields, &long, &mut fonts, &mut images).unwrap();
        assert!(shrunk.h < plain.h, "smaller font: {} vs {}", shrunk.h, plain.h);
    }
}

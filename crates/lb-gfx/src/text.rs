//! Single-line text rendering as vector paths: shadow, outline and fill,
//! plus an optional rounded background plate.

use ab_glyph::{Font, FontArc, OutlineCurve, PxScale, ScaleFont};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use lb_core::graphics::TextStyle;

use crate::color::{parse_color, try_parse};
use crate::layout::rounded_rect;

/// Build a path for `text` with the baseline at y = ascent (top-left origin),
/// `spacing` pixels added after every letter but the last. Returns (path,
/// advance width, line height).
pub fn text_path(font: &FontArc, size: f32, text: &str, spacing: f32) -> (Option<tiny_skia::Path>, f32, f32) {
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let ascent = sf.ascent();
    let height = sf.height();
    let units = font.units_per_em().unwrap_or(1000.0);
    let k = scale.y / units;
    let mut pb = PathBuilder::new();
    let mut x = 0.0f32;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        if let Some(p) = prev {
            x += sf.kern(p, gid);
        }
        if let Some(outline) = font.outline(gid) {
            let mut last: Option<(f32, f32)> = None;
            // glyph space is y-up; flip to screen space
            let tx = |p: ab_glyph::Point| (x + p.x * k, ascent - p.y * k);
            for c in &outline.curves {
                let (start, end) = match c {
                    OutlineCurve::Line(a, b) => (*a, *b),
                    OutlineCurve::Quad(a, _, b) => (*a, *b),
                    OutlineCurve::Cubic(a, _, _, b) => (*a, *b),
                };
                let s = tx(start);
                if last.is_none_or(|l| (l.0 - s.0).abs() > 0.01 || (l.1 - s.1).abs() > 0.01) {
                    if last.is_some() {
                        pb.close();
                    }
                    pb.move_to(s.0, s.1);
                }
                match c {
                    OutlineCurve::Line(_, b) => {
                        let e = tx(*b);
                        pb.line_to(e.0, e.1);
                    }
                    OutlineCurve::Quad(_, c1, b) => {
                        let (c, e) = (tx(*c1), tx(*b));
                        pb.quad_to(c.0, c.1, e.0, e.1);
                    }
                    OutlineCurve::Cubic(_, c1, c2, b) => {
                        let (c1, c2, e) = (tx(*c1), tx(*c2), tx(*b));
                        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
                    }
                }
                last = Some(tx(end));
            }
            if last.is_some() {
                pb.close();
            }
        }
        x += sf.h_advance(gid) + spacing;
        prev = Some(gid);
    }
    if prev.is_some() {
        x -= spacing;
    }
    (pb.finish(), x.max(0.0), height)
}

/// Render styled text into a tightly sized pixmap (including padding,
/// outline and shadow margins).
pub fn render_text(font: &FontArc, style: &TextStyle, text: &str) -> Option<Pixmap> {
    let text = style.apply_case(text);
    let (path, width, height) = text_path(font, style.size, &text, style.letter_spacing);
    let has_bg = style.background.as_deref().and_then(try_parse).is_some();
    let pad = if has_bg { style.padding } else { 0.0 };
    let outline = style.outline_width.max(0.0);
    let (sx, sy) = if style.shadow_color.is_some() { style.shadow_offset } else { (0.0, 0.0) };
    let margin = outline.ceil() + 1.0;
    let w = (width + 2.0 * (pad + margin) + sx.abs()).ceil().max(1.0) as u32;
    let h = (height + 2.0 * (pad + margin) + sy.abs()).ceil().max(1.0) as u32;
    let mut pm = Pixmap::new(w, h)?;
    if let Some(bg) = style.background.as_deref().and_then(try_parse) {
        let mut paint = Paint::default();
        paint.set_color(bg);
        paint.anti_alias = true;
        if let Some(r) = rounded_rect(0.0, 0.0, w as f32, h as f32, style.corner_radius) {
            pm.fill_path(&r, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
    let Some(path) = path else { return Some(pm) };
    let ox = pad + margin + if sx < 0.0 { -sx } else { 0.0 };
    let oy = pad + margin + if sy < 0.0 { -sy } else { 0.0 };
    let base = Transform::from_translate(ox, oy);
    let mut paint = Paint { anti_alias: true, ..Default::default() };
    if let Some(sc) = style.shadow_color.as_deref().and_then(try_parse) {
        paint.set_color(sc);
        pm.fill_path(&path, &paint, FillRule::Winding, base.post_translate(sx, sy), None);
    }
    if outline > 0.0
        && let Some(oc) = style.outline_color.as_deref().and_then(try_parse)
    {
        paint.set_color(oc);
        let stroke = Stroke { width: outline * 2.0, line_join: LineJoin::Round, line_cap: LineCap::Round, ..Default::default() };
        pm.stroke_path(&path, &paint, &stroke, base, None);
    }
    paint.set_color(parse_color(&style.color));
    pm.fill_path(&path, &paint, FillRule::Winding, base, None);
    Some(pm)
}

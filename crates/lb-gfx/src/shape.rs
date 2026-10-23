//! Designed shapes: fill (solid or linear gradient), individual corner
//! radii, slant, soft shadow, accent bar, texture image and border.
//!
//! The pixmap is larger than the shape by the room its shadow and slant
//! need; [`Shape::ox`] / [`Shape::oy`] give where the shape box starts.

use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Point, Rect, Shader,
    SpreadMode, Stroke, Transform,
};

use lb_core::graphics::{ShapeStyle, Side};

use crate::color::{parse_color, try_parse};
use crate::images::ImageCache;

pub struct Shape {
    pub pixmap: Pixmap,
    /// Top-left of the shape box inside the pixmap.
    pub ox: f32,
    pub oy: f32,
}

/// Rounded rectangle with radii (top-left, top-right, bottom-right,
/// bottom-left), each limited to half the shorter side.
pub fn rounded(x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) -> Option<Path> {
    let lim = (w.min(h) / 2.0).max(0.0);
    let [tl, tr, br, bl] = r.map(|v| v.clamp(0.0, lim));
    let mut pb = PathBuilder::new();
    pb.move_to(x + tl, y);
    pb.line_to(x + w - tr, y);
    if tr > 0.0 {
        pb.quad_to(x + w, y, x + w, y + tr);
    }
    pb.line_to(x + w, y + h - br);
    if br > 0.0 {
        pb.quad_to(x + w, y + h, x + w - br, y + h);
    }
    pb.line_to(x + bl, y + h);
    if bl > 0.0 {
        pb.quad_to(x, y + h, x, y + h - bl);
    }
    pb.line_to(x, y + tl);
    if tl > 0.0 {
        pb.quad_to(x, y, x + tl, y);
    }
    pb.close();
    pb.finish()
}

/// Shader for the main colour, or a gradient towards `shape.gradient`.
fn fill_shader(color: &str, shape: &ShapeStyle, x: f32, y: f32, w: f32, h: f32) -> Shader<'static> {
    let from = parse_color(color);
    let Some(to) = shape.gradient.as_deref().and_then(try_parse) else { return Shader::SolidColor(from) };
    let a = shape.gradient_angle.to_radians();
    let (dx, dy) = (a.cos(), a.sin());
    // project the box on the direction so the gradient spans it exactly
    let half = (w * dx.abs() + h * dy.abs()) / 2.0;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let start = Point::from_xy(cx - dx * half, cy - dy * half);
    let end = Point::from_xy(cx + dx * half, cy + dy * half);
    LinearGradient::new(start, end, vec![GradientStop::new(0.0, from), GradientStop::new(1.0, to)], SpreadMode::Pad, Transform::identity())
        .unwrap_or(Shader::SolidColor(from))
}

/// Render a `w` × `h` shape of `color` with corner `radius` and `shape`.
pub fn render_shape(color: &str, w: f32, h: f32, radius: f32, shape: &ShapeStyle, images: &mut ImageCache) -> Option<Shape> {
    let (w, h) = (w.max(1.0), h.max(1.0));
    let (l, t, r, b) = shape.margins(h);
    let mut pm = Pixmap::new((w + l + r).ceil() as u32, (h + t + b).ceil() as u32)?;
    let radii = shape.corners.unwrap_or([radius; 4]);
    let path = rounded(l, t, w, h, radii)?;
    // slant around the vertical centre, so the box keeps its place
    let k = shape.skew.to_radians().tan();
    let slant = Transform::from_row(1.0, 0.0, -k, 1.0, k * (t + h / 2.0), 0.0);
    let path = path.transform(slant)?;

    if let Some(sc) = shape.shadow_color.as_deref().and_then(try_parse) {
        draw_shadow(&mut pm, &path, sc, shape.shadow_blur, shape.shadow_offset);
    }
    let mut paint = Paint { anti_alias: true, ..Default::default() };
    paint.shader = fill_shader(color, shape, l, t, w, h);
    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

    // texture and accent stay inside the shape
    if shape.image.is_some() || shape.accent.is_some() {
        let mut mask = Mask::new(pm.width(), pm.height())?;
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        if let Some(img) = shape.image.as_deref() {
            draw_cover(&mut pm, images, img, l, t, w, h, slant, &mask);
        }
        if let Some(a) = &shape.accent {
            let s = a.size.max(0.0);
            // the bar is drawn wider than the slanted edge and clipped by the mask
            let bar = match a.side {
                Side::Left => Rect::from_xywh(l - w, t, w + s, h),
                Side::Right => Rect::from_xywh(l + w - s, t, w, h),
                Side::Top => Rect::from_xywh(l - w, t, 3.0 * w, s),
                Side::Bottom => Rect::from_xywh(l - w, t + h - s, 3.0 * w, s),
            };
            if let Some(bar) = bar {
                let mut p = Paint { anti_alias: true, ..Default::default() };
                p.set_color(parse_color(&a.color));
                pm.fill_rect(bar, &p, slant, Some(&mask));
            }
        }
    }

    if let Some(bc) = shape.border_color.as_deref().and_then(try_parse)
        && shape.border_width > 0.0
    {
        let mut p = Paint { anti_alias: true, ..Default::default() };
        p.set_color(bc);
        let stroke = Stroke { width: shape.border_width, ..Default::default() };
        // inside the edge, so the shape keeps its size
        let bw = shape.border_width / 2.0;
        let inner = rounded(l + bw, t + bw, (w - 2.0 * bw).max(1.0), (h - 2.0 * bw).max(1.0), radii.map(|v| (v - bw).max(0.0)));
        if let Some(inner) = inner.and_then(|p| p.transform(slant)) {
            pm.stroke_path(&inner, &p, &stroke, Transform::identity(), None);
        }
    }
    Some(Shape { pixmap: pm, ox: l, oy: t })
}

/// Picture scaled to cover the box (cropping the overflow), clipped.
#[allow(clippy::too_many_arguments)]
fn draw_cover(pm: &mut Pixmap, images: &mut ImageCache, img: &str, x: f32, y: f32, w: f32, h: f32, slant: Transform, mask: &Mask) {
    let Some(orig) = images.original(img) else { return };
    let (iw, ih) = (orig.width() as f32, orig.height() as f32);
    let k = (w / iw).max(h / ih);
    let (sw, sh) = (iw * k, ih * k);
    let Some(scaled) = images.scaled(img, sw.round().max(1.0) as u32, sh.round().max(1.0) as u32) else { return };
    let paint = PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
    let t = slant.pre_translate(x + (w - sw) / 2.0, y + (h - sh) / 2.0);
    pm.draw_pixmap(0, 0, scaled.as_ref().as_ref(), &paint, t, Some(mask));
}

fn draw_shadow(pm: &mut Pixmap, path: &Path, color: Color, blur: f32, (dx, dy): (f32, f32)) {
    let Some(mut sh) = Pixmap::new(pm.width(), pm.height()) else { return };
    let mut p = Paint { anti_alias: true, ..Default::default() };
    p.set_color(color);
    sh.fill_path(path, &p, FillRule::Winding, Transform::from_translate(dx, dy), None);
    let r = (blur / 2.0).round() as usize;
    if r > 0 {
        // three box passes approximate a gaussian
        for _ in 0..3 {
            box_blur(sh.data_mut(), pm.width() as usize, pm.height() as usize, r);
        }
    }
    pm.draw_pixmap(0, 0, sh.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

/// Separable box blur of premultiplied RGBA in place.
pub fn box_blur(data: &mut [u8], w: usize, h: usize, r: usize) {
    if w == 0 || h == 0 || r == 0 {
        return;
    }
    let mut tmp = vec![0u8; data.len()];
    pass(data, &mut tmp, w, h, r, true);
    pass(&tmp, data, w, h, r, false);
}

fn pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let idx = |line: usize, i: usize| if horizontal { (line * w + i) * 4 } else { (i * w + line) * 4 };
    let div = (2 * r + 1) as u32;
    for line in 0..lines {
        let mut sum = [0u32; 4];
        // window [i - r, i + r], clamped at the edges
        for k in 0..=r.min(len - 1) {
            let p = idx(line, k);
            for c in 0..4 {
                sum[c] += src[p + c] as u32;
            }
        }
        for i in 0..len {
            let p = idx(line, i);
            for c in 0..4 {
                dst[p + c] = (sum[c] / div) as u8;
            }
            if i + r + 1 < len {
                let q = idx(line, i + r + 1);
                for c in 0..4 {
                    sum[c] += src[q + c] as u32;
                }
            }
            if i >= r {
                let q = idx(line, i - r);
                for c in 0..4 {
                    sum[c] -= src[q + c] as u32;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::graphics::Accent;

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pm.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn gradient_accent_and_border() {
        let mut images = ImageCache::new(std::path::Path::new("."));
        let shape = ShapeStyle {
            gradient: Some("#0000FF".into()),
            accent: Some(Accent { side: Side::Left, size: 10.0, color: "#00FF00".into() }),
            border_color: Some("#FFFFFF".into()),
            border_width: 2.0,
            ..Default::default()
        };
        let s = render_shape("#FF0000", 200.0, 50.0, 0.0, &shape, &mut images).unwrap();
        assert_eq!((s.pixmap.width(), s.pixmap.height(), s.ox), (200, 50, 0.0));
        assert_eq!(px(&s.pixmap, 5, 25), [0, 255, 0, 255], "accent");
        assert_eq!(px(&s.pixmap, 100, 0), [255, 255, 255, 255], "border");
        let left = px(&s.pixmap, 20, 25);
        let right = px(&s.pixmap, 190, 25);
        assert!(left[0] > left[2] && right[2] > right[0], "gradient {left:?} → {right:?}");
    }

    #[test]
    fn shadow_and_slant_grow_the_pixmap() {
        let mut images = ImageCache::new(std::path::Path::new("."));
        let shape = ShapeStyle { skew: 20.0, shadow_color: Some("#000000".into()), shadow_blur: 8.0, shadow_offset: (4.0, 4.0), ..Default::default() };
        let s = render_shape("#FFFFFF", 100.0, 40.0, 0.0, &shape, &mut images).unwrap();
        assert!(s.pixmap.width() > 100 && s.pixmap.height() > 40);
        assert!(s.ox > 0.0 && s.oy > 0.0);
        // slanted: the top edge moved right, the bottom edge left
        let top_left = px(&s.pixmap, s.ox as u32 + 2, s.oy as u32 + 1);
        assert!(top_left[3] < 64, "the slanted corner holds only the soft shadow: {top_left:?}");
    }

    #[test]
    fn blur_spreads() {
        let mut d = vec![0u8; 9 * 4];
        d[4 * 4..4 * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
        box_blur(&mut d, 3, 3, 1);
        assert!(d[3] > 0 && d[4 * 4 + 3] < 255);
    }
}

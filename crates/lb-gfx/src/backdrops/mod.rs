//! Ready-made pictures that come with every channel: full-frame still
//! backgrounds for the library ([`stills`]) and textures for plates and
//! shapes ([`plates`]). They are painted here, deterministically, so they
//! need no files in the program and look the same everywhere.

pub mod plates;
pub mod stills;

use tiny_skia::{Color, GradientStop, LinearGradient, Paint, Pixmap, Point, RadialGradient, Rect, SpreadMode, Transform};

use crate::color::parse_color;

/// A picture with its file stem and title.
pub struct Backdrop {
    /// File name without extension, `bg-midnight`.
    pub stem: &'static str,
    /// Title for the library / picker.
    pub title: &'static str,
}

fn c(hex: &str) -> Color {
    parse_color(hex)
}

fn with_alpha(hex: &str, a: f32) -> Color {
    let mut col = c(hex);
    col.set_alpha(a.clamp(0.0, 1.0));
    col
}

/// Linear gradient over the whole canvas from (x0, y0) to (x1, y1) in
/// fractions of the size.
fn linear(p: &mut Pixmap, from: (f32, f32), to: (f32, f32), stops: &[(f32, Color)]) {
    let (w, h) = (p.width() as f32, p.height() as f32);
    let stops = stops.iter().map(|(o, c)| GradientStop::new(*o, *c)).collect();
    if let Some(shader) = LinearGradient::new(
        Point::from_xy(from.0 * w, from.1 * h),
        Point::from_xy(to.0 * w, to.1 * h),
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        fill(p, shader);
    }
}

/// Soft round glow centred at (x, y) (fractions) with radius `r` (fraction
/// of the width).
fn glow(p: &mut Pixmap, x: f32, y: f32, r: f32, color: Color) {
    let (w, h) = (p.width() as f32, p.height() as f32);
    let mut edge = color;
    edge.set_alpha(0.0);
    let center = Point::from_xy(x * w, y * h);
    if let Some(shader) = RadialGradient::new(
        center,
        0.0,
        center,
        r * w,
        vec![GradientStop::new(0.0, color), GradientStop::new(1.0, edge)],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        fill(p, shader);
    }
}

fn fill(p: &mut Pixmap, shader: tiny_skia::Shader<'static>) {
    let paint = Paint { shader, anti_alias: true, ..Default::default() };
    if let Some(r) = Rect::from_xywh(0.0, 0.0, p.width() as f32, p.height() as f32) {
        p.fill_rect(r, &paint, Transform::identity(), None);
    }
}

fn rect(p: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: Color, t: Transform) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        p.fill_rect(r, &paint, t, None);
    }
}

fn circle(p: &mut Pixmap, x: f32, y: f32, r: f32, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    if let Some(path) = tiny_skia::PathBuilder::from_circle(x, y, r) {
        p.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
    }
}

/// Darker edges (keeps text over the picture readable).
fn vignette(p: &mut Pixmap, strength: f32) {
    let (w, h) = (p.width() as f32, p.height() as f32);
    let center = Point::from_xy(w / 2.0, h / 2.0);
    let stops = vec![GradientStop::new(0.55, with_alpha("#000000", 0.0)), GradientStop::new(1.0, with_alpha("#000000", strength))];
    if let Some(shader) = RadialGradient::new(center, 0.0, center, w.max(h) * 0.75, stops, SpreadMode::Pad, Transform::identity()) {
        fill(p, shader);
    }
}

/// Small deterministic pseudo-random sequence (no RNG dependency).
struct Seq(u32);

impl Seq {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 % 10_000) as f32 / 10_000.0
    }
}

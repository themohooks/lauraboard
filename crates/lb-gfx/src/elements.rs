//! Sprite producers per element kind. A sprite is a small pre-rendered
//! pixmap that the compositor places on the canvas.

use std::collections::HashMap;
use std::sync::Arc;

use tiny_skia::Pixmap;

use lb_core::graphics::{CountdownTarget, Element, ElementKind, FillDirection, ProgressTarget, ShapeStyle, TextStyle, render_template};

use crate::compositor::FrameContext;
use crate::fonts::FontStore;
use crate::images::ImageCache;
use crate::layout::fit_size;
use crate::plate::{PlateLook, render_plate};
use crate::shape::render_shape;
use crate::text::render_text;

/// A pre-rendered element. The pixmap may be larger than the element box
/// (shadow, slant): the box is `w` x `h` at (`ox`, `oy`) inside it.
#[derive(Clone)]
pub struct Sprite {
    pub pixmap: Arc<Pixmap>,
    pub ox: f32,
    pub oy: f32,
    pub w: f32,
    pub h: f32,
}

impl Sprite {
    /// The whole pixmap is the element.
    pub fn plain(pixmap: Arc<Pixmap>) -> Self {
        let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
        Sprite { pixmap, ox: 0.0, oy: 0.0, w, h }
    }
}

/// `strftime`-style formatting that never panics: an invalid pattern
/// (`%Q`, a lone `%` typed while editing) yields `None` instead.
pub fn format_time(now: lb_core::WallTime, pattern: &str) -> Option<String> {
    use std::fmt::Write;
    let mut s = String::new();
    write!(s, "{}", now.format(pattern)).ok()?;
    Some(s)
}

/// Renders and caches sprites.
pub struct SpriteFactory {
    pub fonts: FontStore,
    pub images: ImageCache,
    cache: HashMap<String, Sprite>,
}

const CACHE_LIMIT: usize = 512;

impl SpriteFactory {
    pub fn new(fonts: FontStore, images: ImageCache) -> Self {
        SpriteFactory { fonts, images, cache: HashMap::new() }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.images.clear();
    }

    /// Invalidate changed elements only. Editing a caption must not reload
    /// unrelated logos, backgrounds or pre-rendered plates.
    pub fn update_elements(&mut self, old: &[Element], new: &[Element]) {
        for before in old {
            let after = new.iter().find(|e| e.id == before.id);
            let unchanged = after.is_some_and(|e| {
                e.kind == before.kind && e.placement.width == before.placement.width && e.placement.height == before.placement.height
            });
            if unchanged {
                continue;
            }
            self.cache.retain(|key, _| {
                !["text", "rect", "plate", "progress"].iter().any(|kind| key.starts_with(&format!("{kind}|{}|", before.id)))
            });
            if let ElementKind::Image { path } = &before.kind {
                let same_path = after.is_some_and(|e| matches!(&e.kind, ElementKind::Image { path: next } if next == path));
                self.images.invalidate(path, !same_path);
            }
        }
    }

    /// Sprite for non-ticker, non-group elements.
    pub fn sprite(&mut self, el: &Element, ctx: &FrameContext) -> Option<Sprite> {
        match &el.kind {
            ElementKind::Image { path } => {
                if crate::web::is_web(path) {
                    let w = el.placement.width.unwrap_or(640.0).clamp(1.0, 4096.0) as u32;
                    let h = el.placement.height.unwrap_or(360.0).clamp(1.0, 4096.0) as u32;
                    return self.images.web(path, w, h).frame().map(|f| Sprite::plain(ImageCache::fit(&f, w, h)));
                }
                let orig = self.images.original(path)?;
                let (w, h) = fit_size(&el.placement, orig.width() as f32, orig.height() as f32);
                self.images.scaled(path, w.round().max(1.0) as u32, h.round().max(1.0) as u32).map(Sprite::plain)
            }
            ElementKind::Text { text, style } => {
                let s = render_template(text, ctx.vars);
                self.text(&el.id, style, &s).map(Sprite::plain)
            }
            ElementKind::Clock { format, style, tz } => {
                let s = format_time(lb_core::graphics::clock_time(ctx.now, tz.as_deref()), format)?;
                self.text(&el.id, style, &s).map(Sprite::plain)
            }
            ElementKind::Countdown { target, style, prefix, exclude, format } => {
                let ms = match target {
                    CountdownTarget::ItemRemaining => Some(ctx.item_remaining_ms),
                    CountdownTarget::NextProgram => ctx.next_program_ms,
                    CountdownTarget::ProgramRemaining => {
                        Some(ctx.program.map_or(ctx.item_remaining_ms, |p| p.remaining(exclude, ctx.item_remaining_ms)))
                    }
                }?;
                let s = format!("{}{}", render_template(prefix, ctx.vars), format.format(ms));
                self.text(&el.id, style, &s).map(Sprite::plain)
            }
            ElementKind::Progress { width, height, color, gradient, track, radius, target, exclude, direction, shape } => {
                let w = el.placement.width.unwrap_or(*width).max(1.0);
                let h = el.placement.height.unwrap_or(*height).max(1.0);
                let p = match (target, ctx.program) {
                    (ProgressTarget::Program, Some(clock)) => clock.progress(exclude, ctx.item_remaining_ms),
                    (_, Some(clock)) => {
                        clock.current.map_or(0.0, |(_, d)| if d > 0 { 1.0 - ctx.item_remaining_ms as f32 / d as f32 } else { 0.0 })
                    }
                    (_, None) => 0.0,
                }
                .clamp(0.0, 1.0);
                let along = if matches!(direction, FillDirection::LeftToRight | FillDirection::RightToLeft) { w } else { h };
                // one sprite per filled pixel: the bar moves slowly
                let filled = (p * along).round();
                let key = format!("progress|{}|{w}|{h}|{filled}", el.id);
                if let Some(s) = self.cache.get(&key) {
                    return Some(s.clone());
                }
                let look = ProgressLook {
                    w,
                    h,
                    filled,
                    color,
                    gradient: gradient.as_deref(),
                    track,
                    radius: *radius,
                    direction: *direction,
                    shape,
                };
                let s = render_progress(&look, &mut self.images)?;
                self.store(key, s)
            }
            ElementKind::Rect { color, width, height, radius, shape } => {
                // placement size, when set, wins over the stored one
                let w = el.placement.width.unwrap_or(*width);
                let h = el.placement.height.unwrap_or(*height);
                let key = format!("rect|{}|{w}|{h}", el.id);
                if let Some(s) = self.cache.get(&key) {
                    return Some(s.clone());
                }
                let s = render_shape(color, w, h, *radius, shape, &mut self.images)?;
                let sprite = Sprite { pixmap: Arc::new(s.pixmap), ox: s.ox, oy: s.oy, w: w.max(1.0), h: h.max(1.0) };
                self.store(key, sprite)
            }
            ElementKind::Plate {
                fields,
                color,
                radius,
                shape,
                width,
                min_width,
                padding_x,
                padding_y,
                spacing,
                align,
                layout,
                valign,
                fit,
                icon,
            } => {
                let texts: Vec<String> = fields.iter().map(|f| render_template(&f.text, ctx.vars)).collect();
                let key = format!("plate|{}|{}", el.id, texts.join("\u{1f}"));
                if let Some(s) = self.cache.get(&key) {
                    return Some(s.clone());
                }
                let look = PlateLook {
                    color,
                    radius: *radius,
                    shape,
                    width: el.placement.width.or(*width),
                    min_width: *min_width,
                    min_height: el.placement.height.unwrap_or(0.0),
                    padding_x: *padding_x,
                    padding_y: *padding_y,
                    spacing: *spacing,
                    align: *align,
                    layout: *layout,
                    valign: *valign,
                    fit: *fit,
                    icon: icon.as_ref(),
                };
                let p = render_plate(&look, fields, &texts, &mut self.fonts, &mut self.images)?;
                self.store(key, Sprite { pixmap: Arc::new(p.pixmap), ox: p.ox, oy: p.oy, w: p.w, h: p.h })
            }
            ElementKind::Ticker { .. } | ElementKind::Group { .. } => None,
        }
    }

    /// Rendered text (cached by element, style size and content).
    pub fn text(&mut self, id: &str, style: &TextStyle, s: &str) -> Option<Arc<Pixmap>> {
        if s.is_empty() {
            return None;
        }
        let key = format!("text|{id}|{}|{s}", style.size);
        if let Some(p) = self.cache.get(&key) {
            return Some(p.pixmap.clone());
        }
        let font = self.fonts.get_styled(style.font.as_deref(), style.bold, style.italic)?;
        let pm = Arc::new(render_text(&font, style, s)?);
        self.store(key, Sprite::plain(pm.clone()));
        Some(pm)
    }

    fn store(&mut self, key: String, sprite: Sprite) -> Option<Sprite> {
        if self.cache.len() > CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(key, sprite.clone());
        Some(sprite)
    }
}

struct ProgressLook<'a> {
    w: f32,
    h: f32,
    /// Filled length along the bar, pixels.
    filled: f32,
    color: &'a str,
    gradient: Option<&'a str>,
    track: &'a str,
    radius: f32,
    direction: FillDirection,
    shape: &'a ShapeStyle,
}

/// The bar: the track with its design, the filled part inside its border.
fn render_progress(l: &ProgressLook, images: &mut crate::images::ImageCache) -> Option<Sprite> {
    let track = render_shape(l.track, l.w, l.h, l.radius, l.shape, images)?;
    let mut pm = track.pixmap;
    let inset = l.shape.border_width.max(0.0);
    let (iw, ih) = ((l.w - 2.0 * inset).max(0.0), (l.h - 2.0 * inset).max(0.0));
    let horizontal = matches!(l.direction, FillDirection::LeftToRight | FillDirection::RightToLeft);
    let len = l.filled.min(if horizontal { iw } else { ih });
    if len >= 1.0 && iw >= 1.0 && ih >= 1.0 {
        let (fw, fh) = if horizontal { (len, ih) } else { (iw, len) };
        // the fill keeps the bar's corners and slant, and runs its gradient along it
        let angle = match l.direction {
            FillDirection::LeftToRight => 0.0,
            FillDirection::RightToLeft => 180.0,
            FillDirection::BottomToTop => 270.0,
            FillDirection::TopToBottom => 90.0,
        };
        let fill_shape = ShapeStyle {
            gradient: l.gradient.map(str::to_string),
            gradient_angle: angle,
            corners: l.shape.corners,
            skew: l.shape.skew,
            ..Default::default()
        };
        let r = (l.radius - inset).max(0.0);
        let fill = render_shape(l.color, fw, fh, r, &fill_shape, images)?;
        let (dx, dy) = match l.direction {
            FillDirection::LeftToRight | FillDirection::TopToBottom => (0.0, 0.0),
            FillDirection::RightToLeft => (iw - fw, 0.0),
            FillDirection::BottomToTop => (0.0, ih - fh),
        };
        let x = track.ox + inset + dx - fill.ox;
        let y = track.oy + inset + dy - fill.oy;
        pm.draw_pixmap(
            x.round() as i32,
            y.round() as i32,
            fill.pixmap.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    }
    Some(Sprite { pixmap: Arc::new(pm), ox: track.ox, oy: track.oy, w: l.w, h: l.h })
}

#[cfg(test)]
mod tests {
    #[test]
    fn progress_fills_along_the_bar() {
        let mut images = crate::images::ImageCache::new(std::path::Path::new("."));
        let shape = lb_core::graphics::ShapeStyle::default();
        let look = |filled: f32, direction| super::ProgressLook {
            w: 200.0,
            h: 10.0,
            filled,
            color: "#FF0000",
            gradient: None,
            track: "#00000000",
            radius: 0.0,
            direction,
            shape: &shape,
        };
        let red = |s: &super::Sprite, x: u32| s.pixmap.pixel(x, 5).is_some_and(|p| p.red() > 200 && p.alpha() > 200);
        let half = super::render_progress(&look(100.0, lb_core::graphics::FillDirection::LeftToRight), &mut images).unwrap();
        assert!(red(&half, 10) && !red(&half, 190));
        let back = super::render_progress(&look(100.0, lb_core::graphics::FillDirection::RightToLeft), &mut images).unwrap();
        assert!(!red(&back, 10) && red(&back, 190));
        let none = super::render_progress(&look(0.0, lb_core::graphics::FillDirection::LeftToRight), &mut images).unwrap();
        assert!(!red(&none, 10));
    }

    #[test]
    fn invalid_clock_format_does_not_panic() {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 10, 23).unwrap().and_hms_opt(9, 5, 0).unwrap();
        assert_eq!(super::format_time(t, "%H:%M").as_deref(), Some("09:05"));
        assert_eq!(super::format_time(t, "%H:%Q"), None);
        assert_eq!(super::format_time(t, "%"), None);
    }
}

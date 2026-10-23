//! Sprite producers per element kind. A sprite is a small pre-rendered
//! pixmap that the compositor places on the canvas.

use std::collections::HashMap;
use std::sync::Arc;

use tiny_skia::Pixmap;

use lb_core::graphics::{CountdownTarget, Element, ElementKind, TextStyle, render_template};

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

    /// Sprite for non-ticker, non-group elements.
    pub fn sprite(&mut self, el: &Element, ctx: &FrameContext) -> Option<Sprite> {
        match &el.kind {
            ElementKind::Image { path } => {
                let orig = self.images.original(path)?;
                let (w, h) = fit_size(&el.placement, orig.width() as f32, orig.height() as f32);
                self.images.scaled(path, w.round().max(1.0) as u32, h.round().max(1.0) as u32).map(Sprite::plain)
            }
            ElementKind::Text { text, style } => {
                let s = render_template(text, ctx.vars);
                self.text(&el.id, style, &s).map(Sprite::plain)
            }
            ElementKind::Clock { format, style } => {
                let s = format_time(ctx.now, format)?;
                self.text(&el.id, style, &s).map(Sprite::plain)
            }
            ElementKind::Countdown { target, style, prefix } => {
                let ms = match target {
                    CountdownTarget::ItemRemaining => Some(ctx.item_remaining_ms),
                    CountdownTarget::NextProgram => ctx.next_program_ms,
                }?;
                let s = format!("{}{}", render_template(prefix, ctx.vars), lb_core::fmt_ms(ms.max(0) + 999));
                self.text(&el.id, style, &s).map(Sprite::plain)
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
            ElementKind::Plate { fields, color, radius, shape, width, min_width, padding_x, padding_y, spacing, align } => {
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

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_clock_format_does_not_panic() {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 10, 23).unwrap().and_hms_opt(9, 5, 0).unwrap();
        assert_eq!(super::format_time(t, "%H:%M").as_deref(), Some("09:05"));
        assert_eq!(super::format_time(t, "%H:%Q"), None);
        assert_eq!(super::format_time(t, "%"), None);
    }
}

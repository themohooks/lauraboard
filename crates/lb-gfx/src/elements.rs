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
use crate::text::{render_rect, render_text};

/// Renders and caches sprites.
pub struct SpriteFactory {
    pub fonts: FontStore,
    pub images: ImageCache,
    cache: HashMap<String, Arc<Pixmap>>,
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
    pub fn sprite(&mut self, el: &Element, ctx: &FrameContext) -> Option<Arc<Pixmap>> {
        match &el.kind {
            ElementKind::Image { path } => {
                let orig = self.images.original(path)?;
                let (w, h) = fit_size(&el.placement, orig.width() as f32, orig.height() as f32);
                self.images.scaled(path, w.round().max(1.0) as u32, h.round().max(1.0) as u32)
            }
            ElementKind::Text { text, style } => {
                let s = render_template(text, ctx.vars);
                self.text(&el.id, style, &s)
            }
            ElementKind::Clock { format, style } => {
                let s = ctx.now.format(format).to_string();
                self.text(&el.id, style, &s)
            }
            ElementKind::Countdown { target, style, prefix } => {
                let ms = match target {
                    CountdownTarget::ItemRemaining => Some(ctx.item_remaining_ms),
                    CountdownTarget::NextProgram => ctx.next_program_ms,
                }?;
                let s = format!("{}{}", render_template(prefix, ctx.vars), lb_core::fmt_ms(ms.max(0) + 999));
                self.text(&el.id, style, &s)
            }
            ElementKind::Rect { color, width, height, radius } => {
                let key = format!("rect|{}|{color}|{width}|{height}|{radius}", el.id);
                self.cached(key, || render_rect(color, *width, *height, *radius))
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
            return Some(p.clone());
        }
        let font = self.fonts.get(style.font.as_deref())?;
        self.cached(key, || render_text(&font, style, s))
    }

    fn cached(&mut self, key: String, make: impl FnOnce() -> Option<Pixmap>) -> Option<Arc<Pixmap>> {
        if let Some(p) = self.cache.get(&key) {
            return Some(p.clone());
        }
        if self.cache.len() > CACHE_LIMIT {
            self.cache.clear();
        }
        let p = Arc::new(make()?);
        self.cache.insert(key, p.clone());
        Some(p)
    }
}

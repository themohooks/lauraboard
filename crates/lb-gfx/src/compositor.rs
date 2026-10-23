//! Frame assembly: draws active elements by layer onto a transparent canvas.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use lb_core::graphics::{ActiveGraphic, Element, ElementKind, GraphicsConfig, render_template};
use lb_core::{Ms, WallTime};

use crate::anim;
use crate::elements::SpriteFactory;
use crate::fonts::FontStore;
use crate::images::ImageCache;
use crate::layout::{Bx, place};

/// Everything a frame depends on.
pub struct FrameContext<'a> {
    pub now: WallTime,
    pub active: &'a [ActiveGraphic],
    /// Template variables (`{title}`, `{next_title}`, …).
    pub vars: &'a HashMap<String, String>,
    pub item_remaining_ms: Ms,
    pub next_program_ms: Option<Ms>,
}

pub struct Compositor {
    canvas: Pixmap,
    cfg: Arc<GraphicsConfig>,
    sprites: SpriteFactory,
    /// Whether the last frame was empty (lets callers skip work).
    empty: bool,
    /// Straight-alpha copy of the canvas handed to the encoder.
    out: Vec<u8>,
    /// Tickers on screen: element id → (scroll origin, still visible).
    /// Scrolling runs from the moment a ticker appears, not from the rule's
    /// window (that restarts with every item, and is constant for manual
    /// shows), so the text keeps moving across item changes.
    tickers: HashMap<String, (WallTime, bool)>,
}

impl Compositor {
    pub fn new(width: u32, height: u32, cfg: GraphicsConfig, root: &Path) -> Self {
        Compositor {
            canvas: Pixmap::new(width.max(1), height.max(1)).expect("canvas size"),
            cfg: Arc::new(cfg),
            sprites: SpriteFactory::new(FontStore::new(root), ImageCache::new(root)),
            empty: true,
            out: vec![0; (width.max(1) * height.max(1) * 4) as usize],
            tickers: HashMap::new(),
        }
    }

    /// Replace the graphics configuration (clears caches).
    pub fn set_config(&mut self, cfg: GraphicsConfig) {
        self.cfg = Arc::new(cfg);
        self.sprites.clear();
    }

    /// The last rendered frame (straight-alpha RGBA).
    pub fn last_frame(&self) -> &[u8] {
        &self.out
    }

    /// Reset to a fully transparent frame and return it.
    pub fn clear(&mut self) -> &[u8] {
        self.canvas.fill(tiny_skia::Color::TRANSPARENT);
        self.out.fill(0);
        self.empty = true;
        &self.out
    }

    pub fn width(&self) -> u32 {
        self.canvas.width()
    }

    pub fn height(&self) -> u32 {
        self.canvas.height()
    }

    /// Render a frame; returns straight (non-premultiplied) RGBA bytes, as
    /// expected by ffmpeg's `overlay` filter on YUV video.
    pub fn render(&mut self, ctx: &FrameContext) -> &[u8] {
        let was_empty = self.empty;
        self.render_premultiplied(ctx);
        if self.empty {
            if !was_empty {
                self.out.fill(0);
            }
            return &self.out;
        }
        demultiply_into(self.canvas.data(), &mut self.out);
        &self.out
    }

    /// Render into the internal premultiplied canvas.
    fn render_premultiplied(&mut self, ctx: &FrameContext) {
        if !self.empty || !ctx.active.is_empty() {
            self.canvas.fill(tiny_skia::Color::TRANSPARENT);
        }
        self.empty = ctx.active.is_empty();
        let region = Bx::new(0.0, 0.0, self.canvas.width() as f32, self.canvas.height() as f32);
        let cfg = self.cfg.clone();
        self.tickers.values_mut().for_each(|t| t.1 = false);
        for a in ctx.active {
            if let Some(el) = cfg.element(&a.element) {
                // one broken element must not take the whole overlay down
                let drawn = catch_unwind(AssertUnwindSafe(|| self.draw(&cfg, el, region, ctx, a.shown_ms, a.hide_in_ms, 1.0, 0)));
                if drawn.is_err() {
                    tracing::warn!(element = %el.id, "graphics element failed to render");
                }
            }
        }
        self.tickers.retain(|_, t| t.1);
    }

    /// Current canvas as straight-alpha RGBA PNG (for thumbnails / tests).
    pub fn png(&self) -> Option<Vec<u8>> {
        self.canvas.encode_png().ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        cfg: &GraphicsConfig,
        el: &Element,
        region: Bx,
        ctx: &FrameContext,
        shown: Ms,
        hide_in: Option<Ms>,
        parent_alpha: f32,
        depth: u32,
    ) {
        if depth > 8 {
            return; // protect against cyclic groups
        }
        match &el.kind {
            ElementKind::Group { children } => {
                let w = el.placement.width.unwrap_or(region.w);
                let h = el.placement.height.unwrap_or(region.h);
                let st = anim::state(&el.animation, shown, hide_in, w, h);
                let (x, y) = place(&el.placement, region, w, h);
                let gbox = Bx::new(x + st.dx, y + st.dy, w, h);
                for c in children {
                    if let Some(child) = cfg.element(c) {
                        self.draw(cfg, child, gbox, ctx, shown, hide_in, parent_alpha * el.opacity * st.alpha, depth + 1);
                    }
                }
            }
            ElementKind::Ticker { text, style, speed, width } => {
                let s = render_template(text, ctx.vars);
                let Some(sprite) = self.sprites.text(&el.id, style, &s) else { return };
                let band_w = width.unwrap_or(region.w - 2.0 * el.placement.x).max(1.0);
                let h = sprite.height() as f32;
                let (x, y) = place(&el.placement, region, band_w, h);
                let st = anim::state(&el.animation, shown, hide_in, band_w, h);
                let Some(mut band) = Pixmap::new(band_w as u32, h as u32) else { return };
                let cycle = sprite.width() as f32 + band_w;
                let origin = self.tickers.entry(el.id.clone()).or_insert((lb_core::add_ms(ctx.now, -shown.max(0)), true));
                origin.1 = true;
                let scrolled = lb_core::diff_ms(ctx.now, origin.0).max(0);
                let offset = ((scrolled as f64 / 1000.0) * *speed as f64) % cycle as f64;
                // sub-pixel position: whole-pixel steps alternate between
                // two distances per frame and look jerky
                let sx = band_w as f64 - offset;
                let whole = sx.floor();
                let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
                band.draw_pixmap(
                    whole as i32,
                    0,
                    Pixmap::as_ref(&sprite),
                    &paint,
                    Transform::from_translate((sx - whole) as f32, 0.0),
                    None,
                );
                self.blit(&band, x + st.dx, y + st.dy, parent_alpha * el.opacity * st.alpha);
            }
            _ => {
                let Some(sprite) = self.sprites.sprite(el, ctx) else { return };
                let (x, y) = place(&el.placement, region, sprite.w, sprite.h);
                let st = anim::state(&el.animation, shown, hide_in, sprite.w, sprite.h);
                self.blit(&sprite.pixmap, x - sprite.ox + st.dx, y - sprite.oy + st.dy, parent_alpha * el.opacity * st.alpha);
            }
        }
    }

    fn blit(&mut self, p: &Pixmap, x: f32, y: f32, alpha: f32) {
        if alpha <= 0.001 {
            return;
        }
        let paint = PixmapPaint { opacity: alpha.min(1.0), ..Default::default() };
        self.canvas.draw_pixmap(x.round() as i32, y.round() as i32, p.as_ref(), &paint, Transform::identity(), None);
    }
}

/// Convert premultiplied RGBA to straight alpha.
fn demultiply_into(src: &[u8], dst: &mut [u8]) {
    for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        let a = s[3] as u32;
        match a {
            0 => d.copy_from_slice(&[0, 0, 0, 0]),
            255 => d.copy_from_slice(s),
            _ => {
                d[0] = ((s[0] as u32 * 255 + a / 2) / a).min(255) as u8;
                d[1] = ((s[1] as u32 * 255 + a / 2) / a).min(255) as u8;
                d[2] = ((s[2] as u32 * 255 + a / 2) / a).min(255) as u8;
                d[3] = a as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::graphics::{Anchor, Animation, Placement, TextStyle};

    fn rect(id: &str, color: &str, anchor: Anchor) -> Element {
        Element {
            id: id.into(),
            name: id.into(),
            kind: ElementKind::Rect { color: color.into(), width: 10.0, height: 10.0, radius: 0.0, shape: Default::default() },
            placement: Placement { anchor, x: 0.0, y: 0.0, width: None, height: None },
            opacity: 1.0,
            layer: 0,
            animation: Animation::default(),
            profiles: vec![],
        }
    }

    fn active(id: &str) -> ActiveGraphic {
        ActiveGraphic { element: id.into(), layer: 0, rule: None, priority: 0, shown_ms: 10_000, hide_in_ms: None }
    }

    fn now() -> WallTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(12, 34, 0).unwrap()
    }

    fn px(c: &Compositor, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * c.width() + x) * 4) as usize;
        let d = &c.out;
        [d[i], d[i + 1], d[i + 2], d[i + 3]]
    }

    #[test]
    fn output_is_straight_alpha() {
        let mut r = rect("a", "#C0182B80", Anchor::TopLeft);
        r.opacity = 1.0;
        let cfg = GraphicsConfig { layers: vec![], elements: vec![r], rules: vec![], vars: Default::default() };
        let mut c = Compositor::new(20, 20, cfg, Path::new("."));
        let vars = HashMap::new();
        c.render(&FrameContext { now: now(), active: &[active("a")], vars: &vars, item_remaining_ms: 0, next_program_ms: None });
        let p = px(&c, 2, 2);
        assert!(p[0].abs_diff(0xC0) <= 1 && p[1].abs_diff(0x18) <= 1 && p[3] == 0x80, "{p:?}");
    }

    #[test]
    fn draws_anchored_rects_and_clears() {
        let cfg = GraphicsConfig {
            layers: vec![],
            elements: vec![rect("a", "#FF0000", Anchor::TopLeft), rect("b", "#0000FF", Anchor::BottomRight)],
            rules: vec![],
            vars: Default::default(),
        };
        let mut c = Compositor::new(100, 50, cfg, Path::new("."));
        let vars = HashMap::new();
        let act = vec![active("a"), active("b")];
        c.render(&FrameContext { now: now(), active: &act, vars: &vars, item_remaining_ms: 0, next_program_ms: None });
        assert_eq!(px(&c, 2, 2), [255, 0, 0, 255]);
        assert_eq!(px(&c, 97, 47), [0, 0, 255, 255]);
        assert_eq!(px(&c, 50, 25), [0, 0, 0, 0]);
        c.render(&FrameContext { now: now(), active: &[], vars: &vars, item_remaining_ms: 0, next_program_ms: None });
        assert_eq!(px(&c, 2, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn group_offsets_children() {
        let mut child = rect("c", "#00FF00", Anchor::TopLeft);
        child.placement.x = 5.0;
        let group = Element {
            id: "g".into(),
            name: "g".into(),
            kind: ElementKind::Group { children: vec!["c".into()] },
            placement: Placement { anchor: Anchor::TopLeft, x: 20.0, y: 10.0, width: Some(40.0), height: Some(20.0) },
            opacity: 1.0,
            layer: 0,
            animation: Animation::default(),
            profiles: vec![],
        };
        let cfg = GraphicsConfig { layers: vec![], elements: vec![group, child], rules: vec![], vars: Default::default() };
        let mut c = Compositor::new(100, 50, cfg, Path::new("."));
        let vars = HashMap::new();
        c.render(&FrameContext { now: now(), active: &[active("g")], vars: &vars, item_remaining_ms: 0, next_program_ms: None });
        assert_eq!(px(&c, 26, 12), [0, 255, 0, 255]);
        assert_eq!(px(&c, 22, 12), [0, 0, 0, 0]);
    }

    #[test]
    fn ticker_scrolls_across_item_changes() {
        let el = Element {
            id: "tk".into(),
            name: "tk".into(),
            kind: ElementKind::Ticker {
                text: "TICKER TEXT".into(),
                style: TextStyle { background: Some("#FFFFFF".into()), ..Default::default() },
                speed: 100.0,
                width: None,
            },
            placement: Placement::default(),
            opacity: 1.0,
            layer: 0,
            animation: Animation::default(),
            profiles: vec![],
        };
        let cfg = GraphicsConfig { layers: vec![], elements: vec![el], rules: vec![], vars: Default::default() };
        let mut c = Compositor::new(400, 100, cfg, Path::new("."));
        if c.sprites.fonts.get(None).is_none() {
            return;
        }
        let vars = HashMap::new();
        // leftmost opaque column of the band
        let left_edge = |c: &mut Compositor, at: WallTime, shown: Ms| {
            let act = [ActiveGraphic { shown_ms: shown, ..active("tk") }];
            c.render(&FrameContext { now: at, active: &act, vars: &vars, item_remaining_ms: 0, next_program_ms: None });
            (0..c.width()).find(|x| (0..c.height()).any(|y| px(c, *x, y)[3] > 0)).unwrap_or(c.width())
        };
        let t0 = now();
        let a = left_edge(&mut c, t0, 1_000);
        // a new item started: the rule's window restarts, the text must not
        let b = left_edge(&mut c, lb_core::add_ms(t0, 1_000), 0);
        assert!(b + 50 < a, "ticker should keep moving left: {a} -> {b}");
        // manual show: constant window, still moving
        let d = left_edge(&mut c, lb_core::add_ms(t0, 1_500), 0);
        assert!(d < b, "{b} -> {d}");
    }

    #[test]
    fn text_renders_when_fonts_available() {
        let el = Element {
            id: "t".into(),
            name: "t".into(),
            kind: ElementKind::Clock {
                format: "%H:%M".into(),
                style: TextStyle { background: Some("#000000".into()), ..Default::default() },
            },
            placement: Placement::default(),
            opacity: 1.0,
            layer: 0,
            animation: Animation::default(),
            profiles: vec![],
        };
        let cfg = GraphicsConfig { layers: vec![], elements: vec![el], rules: vec![], vars: Default::default() };
        let mut c = Compositor::new(300, 100, cfg, Path::new("."));
        let vars = HashMap::new();
        c.render(&FrameContext { now: now(), active: &[active("t")], vars: &vars, item_remaining_ms: 0, next_program_ms: None });
        if c.sprites.fonts.get(None).is_some() {
            let opaque = c.out.chunks_exact(4).filter(|p| p[3] == 255).count();
            assert!(opaque > 100, "clock with background should draw pixels");
        }
    }
}

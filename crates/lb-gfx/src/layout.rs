//! Placement maths: anchors, boxes and shapes.

use tiny_skia::{Path, PathBuilder};

use lb_core::graphics::{Anchor, Placement};

/// A rectangle in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bx {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bx {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Bx { x, y, w, h }
    }
}

/// Top-left position of an element of size (w, h) placed in `region`.
pub fn place(p: &Placement, region: Bx, w: f32, h: f32) -> (f32, f32) {
    let (fx, fy) = match p.anchor {
        Anchor::TopLeft => (0.0, 0.0),
        Anchor::TopCenter => (0.5, 0.0),
        Anchor::TopRight => (1.0, 0.0),
        Anchor::CenterLeft => (0.0, 0.5),
        Anchor::Center => (0.5, 0.5),
        Anchor::CenterRight => (1.0, 0.5),
        Anchor::BottomLeft => (0.0, 1.0),
        Anchor::BottomCenter => (0.5, 1.0),
        Anchor::BottomRight => (1.0, 1.0),
    };
    // margins push away from the anchored edge; centred axes treat x/y as offsets
    let dx = if fx == 1.0 { -p.x } else { p.x };
    let dy = if fy == 1.0 { -p.y } else { p.y };
    (region.x + (region.w - w) * fx + dx, region.y + (region.h - h) * fy + dy)
}

/// Target size honouring placement width/height (keeping aspect when only
/// one is set).
pub fn fit_size(p: &Placement, nat_w: f32, nat_h: f32) -> (f32, f32) {
    match (p.width, p.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) if nat_w > 0.0 => (w, nat_h * w / nat_w),
        (None, Some(h)) if nat_h > 0.0 => (nat_w * h / nat_h, h),
        _ => (nat_w, nat_h),
    }
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    if r <= 0.0 {
        pb.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
        return pb.finish();
    }
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors() {
        let r = Bx::new(0.0, 0.0, 1000.0, 500.0);
        let p = |a, x, y| Placement { anchor: a, x, y, width: None, height: None };
        assert_eq!(place(&p(Anchor::TopLeft, 10.0, 20.0), r, 100.0, 50.0), (10.0, 20.0));
        assert_eq!(place(&p(Anchor::TopRight, 10.0, 20.0), r, 100.0, 50.0), (890.0, 20.0));
        assert_eq!(place(&p(Anchor::BottomRight, 10.0, 20.0), r, 100.0, 50.0), (890.0, 430.0));
        assert_eq!(place(&p(Anchor::Center, 0.0, 0.0), r, 100.0, 50.0), (450.0, 225.0));
    }

    #[test]
    fn sizes() {
        let p = Placement { width: Some(200.0), ..Default::default() };
        assert_eq!(fit_size(&p, 100.0, 50.0), (200.0, 100.0));
    }
}

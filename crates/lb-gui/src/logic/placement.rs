//! Graphics canvas geometry: element boxes in frame pixels (mirrors
//! `lb_gfx::layout::place` / `fit_size`), inverse mapping for drag and
//! resize, snapping to frame edges, centre and the safe area.

use lb_core::graphics::{Anchor, Element, ElementKind, Placement, TextStyle};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Snap result: adjusted rect plus guide lines to draw (frame px).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapped {
    pub rect: Rect,
    pub vguides: Vec<f32>,
    pub hguides: Vec<f32>,
}

fn anchor_factors(a: Anchor) -> (f32, f32) {
    match a {
        Anchor::TopLeft => (0.0, 0.0),
        Anchor::TopCenter => (0.5, 0.0),
        Anchor::TopRight => (1.0, 0.0),
        Anchor::CenterLeft => (0.0, 0.5),
        Anchor::Center => (0.5, 0.5),
        Anchor::CenterRight => (1.0, 0.5),
        Anchor::BottomLeft => (0.0, 1.0),
        Anchor::BottomCenter => (0.5, 1.0),
        Anchor::BottomRight => (1.0, 1.0),
    }
}

pub const ANCHORS: [Anchor; 9] = [
    Anchor::TopLeft,
    Anchor::TopCenter,
    Anchor::TopRight,
    Anchor::CenterLeft,
    Anchor::Center,
    Anchor::CenterRight,
    Anchor::BottomLeft,
    Anchor::BottomCenter,
    Anchor::BottomRight,
];

fn text_box(sample: &str, st: &TextStyle) -> (f32, f32) {
    let chars = sample.chars().count().max(1) as f32;
    (chars * st.size * 0.55 + 2.0 * st.padding, st.size * 1.25 + 2.0 * st.padding)
}

/// Approximate natural size (exact sizes come from the server render).
pub fn natural_size(e: &Element, frame_w: f32) -> (f32, f32) {
    match &e.kind {
        ElementKind::Text { text, style } => text_box(text, style),
        ElementKind::Ticker { style, width, .. } => (width.unwrap_or(frame_w), text_box("x", style).1),
        ElementKind::Clock { format, style } => text_box(&format.replace("%H", "00").replace("%M", "00").replace("%S", "00"), style),
        ElementKind::Countdown { style, prefix, .. } => text_box(&format!("{prefix}00:00:00"), style),
        ElementKind::Rect { width, height, .. } => (*width, *height),
        ElementKind::Image { .. } => (320.0, 180.0),
        ElementKind::Group { .. } => (400.0, 120.0),
    }
}

/// Element box in the frame.
pub fn rect_of(e: &Element, frame_w: f32, frame_h: f32) -> Rect {
    let (nw, nh) = natural_size(e, frame_w);
    let p = &e.placement;
    let (w, h) = match (p.width, p.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) if nw > 0.0 => (w, nh * w / nw),
        (None, Some(h)) if nh > 0.0 => (nw * h / nh, h),
        _ => (nw, nh),
    };
    let (fx, fy) = anchor_factors(p.anchor);
    let dx = if fx == 1.0 { -p.x } else { p.x };
    let dy = if fy == 1.0 { -p.y } else { p.y };
    Rect { x: (frame_w - w) * fx + dx, y: (frame_h - h) * fy + dy, w, h }
}

/// Placement x/y that puts the box's top-left at `r` for the given anchor.
pub fn placement_for(mut p: Placement, r: Rect, frame_w: f32, frame_h: f32) -> Placement {
    let (fx, fy) = anchor_factors(p.anchor);
    let base_x = (frame_w - r.w) * fx;
    let base_y = (frame_h - r.h) * fy;
    p.x = if fx == 1.0 { base_x - r.x } else { r.x - base_x };
    p.y = if fy == 1.0 { base_y - r.y } else { r.y - base_y };
    p.x = (p.x * 10.0).round() / 10.0;
    p.y = (p.y * 10.0).round() / 10.0;
    p
}

/// Snap a box to frame edges, centre and the 5 % safe area.
pub fn snap(r: Rect, frame_w: f32, frame_h: f32, threshold: f32) -> Snapped {
    let mut out = Snapped { rect: r, ..Default::default() };
    let sx = frame_w * 0.05;
    let sy = frame_h * 0.05;
    // candidate (guide, resulting x)
    let xs = [(0.0, 0.0), (sx, sx), (frame_w / 2.0, (frame_w - r.w) / 2.0), (frame_w - sx, frame_w - sx - r.w), (frame_w, frame_w - r.w)];
    let ys = [(0.0, 0.0), (sy, sy), (frame_h / 2.0, (frame_h - r.h) / 2.0), (frame_h - sy, frame_h - sy - r.h), (frame_h, frame_h - r.h)];
    if let Some((g, x)) = xs.iter().copied().min_by(|a, b| (a.1 - r.x).abs().total_cmp(&(b.1 - r.x).abs()))
        && (x - r.x).abs() <= threshold
    {
        out.rect.x = x;
        out.vguides.push(g);
    }
    if let Some((g, y)) = ys.iter().copied().min_by(|a, b| (a.1 - r.y).abs().total_cmp(&(b.1 - r.y).abs()))
        && (y - r.y).abs() <= threshold
    {
        out.rect.y = y;
        out.hguides.push(g);
    }
    out
}

/// Resize from a handle (0 tl, 1 t, 2 tr, 3 r, 4 br, 5 b, 6 bl, 7 l) by (dx, dy).
pub fn resize(r: Rect, handle: i32, dx: f32, dy: f32, min: f32) -> Rect {
    let (mut l, mut t, mut rr, mut b) = (r.x, r.y, r.x + r.w, r.y + r.h);
    match handle {
        0 => {
            l += dx;
            t += dy;
        }
        1 => t += dy,
        2 => {
            rr += dx;
            t += dy;
        }
        3 => rr += dx,
        4 => {
            rr += dx;
            b += dy;
        }
        5 => b += dy,
        6 => {
            l += dx;
            b += dy;
        }
        _ => l += dx,
    }
    if rr - l < min {
        if matches!(handle, 0 | 6 | 7) { l = rr - min } else { rr = l + min }
    }
    if b - t < min {
        if matches!(handle, 0 | 1 | 2) { t = b - min } else { b = t + min }
    }
    Rect { x: l, y: t, w: rr - l, h: b - t }
}

/// `#RRGGBB` / `#RRGGBBAA` check for colour fields.
pub fn valid_color(s: &str) -> bool {
    let h = s.trim().trim_start_matches('#');
    s.trim().starts_with('#') && (h.len() == 6 || h.len() == 8) && h.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_el(anchor: Anchor, x: f32, y: f32) -> Element {
        serde_json::from_value(serde_json::json!({
            "id": "r", "type": "rect", "color": "#ffffff", "width": 100.0, "height": 50.0,
            "placement": { "anchor": serde_json::to_value(anchor).unwrap(), "x": x, "y": y }
        }))
        .unwrap()
    }

    #[test]
    fn matches_server_layout() {
        // same expectations as lb_gfx::layout tests (1000x500 frame)
        assert_eq!(rect_of(&rect_el(Anchor::TopRight, 10.0, 20.0), 1000.0, 500.0), Rect { x: 890.0, y: 20.0, w: 100.0, h: 50.0 });
        assert_eq!(rect_of(&rect_el(Anchor::BottomRight, 10.0, 20.0), 1000.0, 500.0), Rect { x: 890.0, y: 430.0, w: 100.0, h: 50.0 });
        assert_eq!(rect_of(&rect_el(Anchor::Center, 0.0, 0.0), 1000.0, 500.0), Rect { x: 450.0, y: 225.0, w: 100.0, h: 50.0 });
    }

    #[test]
    fn placement_roundtrip_for_every_anchor() {
        for a in ANCHORS {
            let e = rect_el(a, 0.0, 0.0);
            let target = Rect { x: 123.0, y: 77.0, w: 100.0, h: 50.0 };
            let mut e2 = e.clone();
            e2.placement = placement_for(e.placement.clone(), target, 1920.0, 1080.0);
            assert_eq!(rect_of(&e2, 1920.0, 1080.0), target, "{a:?}");
        }
    }

    #[test]
    fn snapping_and_resizing() {
        let s = snap(Rect { x: 5.0, y: 537.0, w: 100.0, h: 10.0 }, 1920.0, 1080.0, 8.0);
        assert_eq!(s.rect.x, 0.0);
        assert_eq!(s.rect.y, 535.0);
        assert_eq!(s.hguides, vec![540.0]);
        let r = resize(Rect { x: 10.0, y: 10.0, w: 100.0, h: 100.0 }, 0, 200.0, 0.0, 20.0);
        assert_eq!((r.x, r.w), (90.0, 20.0));
        assert!(valid_color("#FF00aa80") && !valid_color("red"));
    }
}

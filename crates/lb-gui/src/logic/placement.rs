//! Graphics canvas geometry: element boxes in frame pixels (mirrors
//! `lb_gfx::layout::place` / `fit_size`), inverse mapping for drag and
//! resize, snapping to frame edges, centre and the safe area.

use lb_core::graphics::{Anchor, Element, ElementKind, GraphicsConfig, Placement, TextStyle};

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

/// Approximate natural size inside `region` (exact sizes come from the
/// server render).
fn natural_size(e: &Element, region: Rect) -> (f32, f32) {
    match &e.kind {
        ElementKind::Text { text, style } => text_box(text, style),
        ElementKind::Ticker { style, width, .. } => (width.unwrap_or(region.w - 2.0 * e.placement.x).max(1.0), text_box("x", style).1),
        ElementKind::Clock { format, style } => text_box(&format.replace("%H", "00").replace("%M", "00").replace("%S", "00"), style),
        ElementKind::Countdown { style, prefix, .. } => text_box(&format!("{prefix}00:00:00"), style),
        ElementKind::Rect { width, height, .. } => (*width, *height),
        ElementKind::Image { .. } => (320.0, 180.0),
        // a group without a size fills its region, like the compositor
        ElementKind::Group { .. } => (region.w, region.h),
    }
}

/// Element box in the frame.
#[cfg(test)]
pub fn rect_of(e: &Element, frame_w: f32, frame_h: f32) -> Rect {
    rect_in(e, Rect { x: 0.0, y: 0.0, w: frame_w, h: frame_h })
}

/// Element box inside `region` (the frame, or the box of its group).
pub fn rect_in(e: &Element, region: Rect) -> Rect {
    let (nw, nh) = natural_size(e, region);
    let p = &e.placement;
    let (w, h) = match (&e.kind, p.width, p.height) {
        (ElementKind::Group { .. }, w, h) => (w.unwrap_or(nw), h.unwrap_or(nh)),
        (ElementKind::Ticker { .. }, _, _) => (nw, nh),
        (_, Some(w), Some(h)) => (w, h),
        (_, Some(w), None) if nw > 0.0 => (w, nh * w / nw),
        (_, None, Some(h)) if nh > 0.0 => (nw * h / nh, h),
        _ => (nw, nh),
    };
    let (fx, fy) = anchor_factors(p.anchor);
    let dx = if fx == 1.0 { -p.x } else { p.x };
    let dy = if fy == 1.0 { -p.y } else { p.y };
    Rect { x: region.x + (region.w - w) * fx + dx, y: region.y + (region.h - h) * fy + dy, w, h }
}

/// Region an element is placed in: the box of the group listing it as a
/// child (groups nest, at most 8 deep like the compositor), else the frame.
pub fn region_of(cfg: &GraphicsConfig, id: &str, frame_w: f32, frame_h: f32) -> Rect {
    let frame = Rect { x: 0.0, y: 0.0, w: frame_w, h: frame_h };
    let mut chain = Vec::new();
    let mut cur = id;
    while chain.len() < 8 {
        let parent = cfg.elements.iter().find(|g| matches!(&g.kind, ElementKind::Group { children } if children.iter().any(|c| c == cur)));
        match parent {
            Some(g) if !chain.iter().any(|x: &&Element| x.id == g.id) && g.id != id => {
                chain.push(g);
                cur = &g.id;
            }
            _ => break,
        }
    }
    chain.iter().rev().fold(frame, |region, g| rect_in(g, region))
}

/// Placement x/y that puts the box's top-left at `r` for the given anchor.
#[cfg(test)]
pub fn placement_for(p: Placement, r: Rect, frame_w: f32, frame_h: f32) -> Placement {
    placement_in(p, r, Rect { x: 0.0, y: 0.0, w: frame_w, h: frame_h })
}

/// Inverse of [`rect_in`]: placement putting the box at `r` inside `region`.
pub fn placement_in(mut p: Placement, r: Rect, region: Rect) -> Placement {
    let (fx, fy) = anchor_factors(p.anchor);
    let base_x = region.x + (region.w - r.w) * fx;
    let base_y = region.y + (region.h - r.h) * fy;
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
        if matches!(handle, 0..=2) { t = b - min } else { b = t + min }
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

    #[test]
    fn group_children_are_placed_in_the_group_box() {
        let mut g = rect_el(Anchor::BottomLeft, 40.0, 60.0);
        g.id = "grp".into();
        g.kind = ElementKind::Group { children: vec!["kid".into()] };
        g.placement.width = Some(600.0);
        g.placement.height = Some(120.0);
        let mut kid = rect_el(Anchor::TopRight, 10.0, 20.0);
        kid.id = "kid".into();
        let cfg = GraphicsConfig { elements: vec![g.clone(), kid.clone()], ..Default::default() };
        let region = region_of(&cfg, "kid", 1920.0, 1080.0);
        assert_eq!(region, Rect { x: 40.0, y: 900.0, w: 600.0, h: 120.0 });
        assert_eq!(rect_in(&kid, region), Rect { x: 530.0, y: 920.0, w: 100.0, h: 50.0 });
        assert_eq!(region_of(&cfg, "grp", 1920.0, 1080.0), Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 });
        // inverse inside the region
        let p = placement_in(kid.placement.clone(), Rect { x: 500.0, y: 910.0, w: 100.0, h: 50.0 }, region);
        assert_eq!((p.x, p.y), (40.0, 10.0));
    }
}

//! In/out animations: opacity, offset, scale, rotation and a visible part
//! of an element at a given moment.
//!
//! Directions are directions of movement: `slide_left` comes in from the
//! right moving left, and goes out moving left. Other kinds play the same
//! motion backwards on the way out (`zoom_in` grows in, shrinks out).

use lb_core::graphics::{AnimKind, Animation};

/// Visual state produced by an animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimState {
    pub alpha: f32,
    pub dx: f32,
    pub dy: f32,
    /// Scale around the element's centre.
    pub sx: f32,
    pub sy: f32,
    /// Rotation around the centre, degrees.
    pub rotate: f32,
    /// Visible part in element fractions (x0, y0, x1, y1).
    pub clip: Option<[f32; 4]>,
}

impl AnimState {
    pub const IDLE: AnimState = AnimState { alpha: 1.0, dx: 0.0, dy: 0.0, sx: 1.0, sy: 1.0, rotate: 0.0, clip: None };

    /// Only alpha and offset (no scale, rotation or clipping).
    pub fn is_simple(&self) -> bool {
        self.sx == 1.0 && self.sy == 1.0 && self.rotate == 0.0 && self.clip.is_none()
    }

    fn fade(alpha: f32) -> Self {
        AnimState { alpha, ..Self::IDLE }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn out_cubic(t: f32) -> f32 {
    let t = 1.0 - t.clamp(0.0, 1.0);
    1.0 - t * t * t
}

/// Overshoots past 1 and settles.
fn out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    let c = 1.70158;
    1.0 + t * t * ((c + 1.0) * t + c)
}

fn out_bounce(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// `progress` 0 = hidden, 1 = fully shown; `out`: on the way out
/// (progress runs from 1 down to 0).
fn apply(kind: AnimKind, progress: f32, out: bool, w: f32, h: f32) -> AnimState {
    use AnimKind as K;
    let p = smooth(progress);
    let inv = 1.0 - p;
    // movement kinds keep their direction on the way out
    let dir = if out { -1.0 } else { 1.0 };
    let short = 0.25;
    let mv = |dx: f32, dy: f32, alpha: f32| AnimState { alpha, dx, dy, ..AnimState::IDLE };
    match kind {
        K::None => AnimState::IDLE,
        K::Fade => AnimState::fade(p),
        K::SlideLeft => mv(dir * inv * w, 0.0, p),
        K::SlideRight => mv(-dir * inv * w, 0.0, p),
        K::SlideUp => mv(0.0, dir * inv * h, p),
        K::SlideDown => mv(0.0, -dir * inv * h, p),
        K::FadeLeft => mv(dir * inv * w * short, 0.0, p),
        K::FadeRight => mv(-dir * inv * w * short, 0.0, p),
        K::FadeUp => mv(0.0, dir * inv * h.max(40.0) * short * 2.0, p),
        K::FadeDown => mv(0.0, -dir * inv * h.max(40.0) * short * 2.0, p),
        K::ZoomIn => {
            let s = 0.5 + 0.5 * out_cubic(progress);
            AnimState { alpha: p, sx: s, sy: s, ..AnimState::IDLE }
        }
        K::ZoomOut => {
            let s = 1.0 + 0.5 * (1.0 - out_cubic(progress));
            AnimState { alpha: p, sx: s, sy: s, ..AnimState::IDLE }
        }
        K::Pop => {
            let s = out_back(progress).max(0.01);
            AnimState { alpha: smooth(progress * 2.0), sx: s, sy: s, ..AnimState::IDLE }
        }
        K::WipeLeft | K::WipeRight | K::WipeUp | K::WipeDown => {
            // the visible part grows in the direction; going out its
            // trailing edge follows the same way
            let v = out_cubic(progress);
            let forward = matches!(kind, K::WipeRight | K::WipeDown) != out;
            let (a, b) = if forward { (0.0, v) } else { (1.0 - v, 1.0) };
            let clip = if matches!(kind, K::WipeLeft | K::WipeRight) { [a, 0.0, b, 1.0] } else { [0.0, a, 1.0, b] };
            AnimState { clip: Some(clip), ..AnimState::IDLE }
        }
        K::StretchX => AnimState { sx: out_cubic(progress).max(0.01), alpha: smooth(progress * 3.0), ..AnimState::IDLE },
        K::StretchY => AnimState { sy: out_cubic(progress).max(0.01), alpha: smooth(progress * 3.0), ..AnimState::IDLE },
        K::BounceUp => mv(0.0, (1.0 - out_bounce(progress)) * h.max(40.0) * 1.5, smooth(progress * 4.0)),
        K::DropDown => mv(0.0, -(1.0 - out_bounce(progress)) * h.max(40.0) * 1.5, smooth(progress * 4.0)),
        K::Spin => {
            let s = 0.6 + 0.4 * out_cubic(progress);
            AnimState { alpha: p, sx: s, sy: s, rotate: -90.0 * (1.0 - out_cubic(progress)), ..AnimState::IDLE }
        }
        K::Flicker => {
            // three blinks, the last one stays
            let t = progress.clamp(0.0, 1.0);
            let on = t >= 0.85 || ((t * 7.0) as i32) % 2 == 1;
            AnimState::fade(if on { 1.0 } else { 0.15 * t })
        }
    }
}

/// Combine in- and out-animations for an element of size (w, h).
pub fn state(a: &Animation, shown_ms: i64, hide_in_ms: Option<i64>, w: f32, h: f32) -> AnimState {
    let d = a.duration.ms().max(1) as f32;
    if let Some(left) = hide_in_ms
        && a.anim_out != AnimKind::None
        && (left as f32) < d
    {
        return apply(a.anim_out, left as f32 / d, true, w, h);
    }
    if a.anim_in != AnimKind::None && (shown_ms as f32) < d {
        return apply(a.anim_in, shown_ms as f32 / d, false, w, h);
    }
    AnimState::IDLE
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::Dur;

    fn anim(i: AnimKind, o: AnimKind) -> Animation {
        Animation { anim_in: i, anim_out: o, duration: Dur(1000) }
    }

    #[test]
    fn fade_in_and_out() {
        let a = anim(AnimKind::Fade, AnimKind::SlideLeft);
        assert_eq!(state(&a, 0, None, 100.0, 10.0).alpha, 0.0);
        assert!((state(&a, 500, None, 100.0, 10.0).alpha - 0.5).abs() < 1e-6);
        assert_eq!(state(&a, 5000, None, 100.0, 10.0), AnimState::IDLE);
        let out = state(&a, 5000, Some(0), 100.0, 10.0);
        assert_eq!(out.alpha, 0.0);
        // leaves moving left
        assert_eq!(out.dx, -100.0);
    }

    #[test]
    fn slide_comes_from_the_opposite_side() {
        let a = anim(AnimKind::SlideRight, AnimKind::None);
        assert_eq!(state(&a, 0, None, 100.0, 10.0).dx, -100.0);
    }

    #[test]
    fn every_kind_ends_idle_and_starts_hidden_or_clipped() {
        for k in AnimKind::ALL {
            let a = anim(k, k);
            assert_eq!(state(&a, 1000, None, 100.0, 50.0), AnimState::IDLE, "{k:?} settled");
            let s = state(&a, 0, None, 100.0, 50.0);
            let hidden = s.alpha <= 0.2 || s.clip.is_some_and(|c| (c[2] - c[0]) * (c[3] - c[1]) < 0.01) || s.sx < 0.05 || s.sy < 0.05;
            assert!(k == AnimKind::None || hidden, "{k:?} starts hidden: {s:?}");
        }
    }

    #[test]
    fn pop_overshoots() {
        let a = anim(AnimKind::Pop, AnimKind::None);
        assert!((1..10).map(|i| state(&a, i * 100, None, 10.0, 10.0).sx).any(|s| s > 1.0));
    }

    #[test]
    fn wipe_right_reveals_from_the_left() {
        let a = anim(AnimKind::WipeRight, AnimKind::WipeRight);
        let c = state(&a, 500, None, 100.0, 10.0).clip.unwrap();
        assert_eq!(c[0], 0.0);
        assert!(c[2] > 0.0 && c[2] < 1.0);
        // going out the left edge moves right
        let c = state(&a, 5000, Some(500), 100.0, 10.0).clip.unwrap();
        assert!(c[0] > 0.0 && c[2] == 1.0);
    }
}

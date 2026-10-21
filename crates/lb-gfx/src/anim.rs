//! In/out animations: opacity and offset for an element at a given moment.

use lb_core::graphics::{AnimKind, Animation};

/// Visual state produced by an animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimState {
    pub alpha: f32,
    pub dx: f32,
    pub dy: f32,
}

impl AnimState {
    pub const IDLE: AnimState = AnimState { alpha: 1.0, dx: 0.0, dy: 0.0 };
}

fn ease(t: f32) -> f32 {
    // smoothstep
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `progress` 0 = hidden, 1 = fully shown.
fn apply(kind: AnimKind, progress: f32, w: f32, h: f32) -> AnimState {
    let p = ease(progress);
    let inv = 1.0 - p;
    match kind {
        AnimKind::None => AnimState::IDLE,
        AnimKind::Fade => AnimState { alpha: p, dx: 0.0, dy: 0.0 },
        AnimKind::SlideLeft => AnimState { alpha: p, dx: inv * w, dy: 0.0 },
        AnimKind::SlideRight => AnimState { alpha: p, dx: -inv * w, dy: 0.0 },
        AnimKind::SlideUp => AnimState { alpha: p, dx: 0.0, dy: inv * h },
        AnimKind::SlideDown => AnimState { alpha: p, dx: 0.0, dy: -inv * h },
    }
}

/// Combine in- and out-animations for an element of size (w, h).
pub fn state(a: &Animation, shown_ms: i64, hide_in_ms: Option<i64>, w: f32, h: f32) -> AnimState {
    let d = a.duration.ms().max(1) as f32;
    if let Some(left) = hide_in_ms
        && a.anim_out != AnimKind::None
        && (left as f32) < d
    {
        return apply(a.anim_out, left as f32 / d, w, h);
    }
    if a.anim_in != AnimKind::None && (shown_ms as f32) < d {
        return apply(a.anim_in, shown_ms as f32 / d, w, h);
    }
    AnimState::IDLE
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::Dur;

    #[test]
    fn fade_in_and_out() {
        let a = Animation { anim_in: AnimKind::Fade, anim_out: AnimKind::SlideLeft, duration: Dur(1000) };
        assert_eq!(state(&a, 0, None, 100.0, 10.0).alpha, 0.0);
        assert!((state(&a, 500, None, 100.0, 10.0).alpha - 0.5).abs() < 1e-6);
        assert_eq!(state(&a, 5000, None, 100.0, 10.0), AnimState::IDLE);
        let out = state(&a, 5000, Some(0), 100.0, 10.0);
        assert_eq!(out.alpha, 0.0);
        assert_eq!(out.dx, 100.0);
    }
}

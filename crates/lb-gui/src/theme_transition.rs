//! Theme change: the new theme spreads in a circle from the clicked
//! button. Two still pictures (old and new theme) are uploaded once; the
//! circle itself is a Slint animation (a rounded clip), drawn by the GPU
//! at the display's refresh rate.

use std::time::Duration;

use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

use crate::shell::AppWindow;

/// How long the circle grows (the animation in `app.slint` matches it).
pub const DURATION: Duration = Duration::from_millis(520);

/// Show `old` and let `new` grow over it from `click` (window pixels;
/// `None`: from the middle).
pub fn start(ui: &AppWindow, old: SharedPixelBuffer<Rgba8Pixel>, new: SharedPixelBuffer<Rgba8Pixel>, click: Option<(f32, f32)>) {
    let scale = ui.window().scale_factor().max(0.1);
    let (w, h) = (old.width() as f32 / scale, old.height() as f32 / scale);
    let (x, y) = click.map_or((w / 2.0, h / 2.0), |(x, y)| ((x / scale).clamp(0.0, w), (y / scale).clamp(0.0, h)));
    ui.set_theme_origin_x(x);
    ui.set_theme_origin_y(y);
    ui.set_theme_radius(radius(w, h, x, y));
    ui.set_theme_snapshot(Image::from_rgba8(old));
    ui.set_theme_next(Image::from_rgba8(new));
    // the overlay's clock starts at the last frame's time: bring it up to
    // now, the two snapshots took a while and the circle would jump ahead
    slint::platform::update_timers_and_animations();
    ui.set_theme_transition(true);
}

/// Drop the pictures (the live interface already has the new theme).
pub fn finish(ui: &AppWindow) {
    ui.set_theme_transition(false);
    ui.set_theme_snapshot(Image::default());
    ui.set_theme_next(Image::default());
}

/// Distance from (x, y) to the farthest corner: the circle covers the
/// whole window at the end.
fn radius(w: f32, h: f32, x: f32, y: f32) -> f32 {
    x.max(w - x).hypot(y.max(h - y)) + 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_reaches_the_farthest_corner() {
        assert_eq!(radius(400.0, 300.0, 400.0, 0.0), 501.0);
        assert_eq!(radius(400.0, 300.0, 200.0, 150.0), 251.0);
    }
}

//! Raster alpha mask: rounded clipping is unsupported by Slint's software renderer.
//! The snapshot of the old theme lies over the new one; a growing circle
//! is cut out of it, so the new theme spreads from the clicked button.
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub(crate) struct Reveal {
    pixels: SharedPixelBuffer<Rgba8Pixel>,
    /// Already cleared part of each row (empty: start == end).
    cleared: Vec<(usize, usize)>,
    origin: (f32, f32),
    radius: f32,
}

impl Reveal {
    /// `origin` in snapshot pixels; `None`: the middle of the window.
    pub fn new(pixels: SharedPixelBuffer<Rgba8Pixel>, origin: Option<(f32, f32)>) -> Self {
        let (w, h) = (pixels.width() as f32, pixels.height() as f32);
        let (cx, cy) = origin.map_or((w / 2.0, h / 2.0), |(x, y)| (x.clamp(0.0, w), y.clamp(0.0, h)));
        // reach the farthest corner
        let radius = (cx.max(w - cx)).hypot(cy.max(h - cy)) + 1.0;
        let start = cx as usize;
        Self { cleared: vec![(start, start); pixels.height() as usize], pixels, origin: (cx, cy), radius }
    }

    /// Progress is monotonic. Only the newly uncovered pixels are cleared.
    pub fn frame(&mut self, progress: f32) -> Image {
        let width = self.pixels.width() as usize;
        let (cx, cy) = self.origin;
        let radius = self.radius * progress.clamp(0.0, 1.0);
        let pixels = self.pixels.make_mut_slice();
        for (y, span) in self.cleared.iter_mut().enumerate() {
            let dy = y as f32 + 0.5 - cy;
            let half = (radius * radius - dy * dy).max(0.0).sqrt();
            if half == 0.0 {
                continue;
            }
            let left = ((cx - half).floor().max(0.0) as usize).min(span.0);
            let right = ((cx + half).ceil() as usize).min(width).max(span.1);
            let row = &mut pixels[y * width..(y + 1) * width];
            row[left..span.0].fill(Rgba8Pixel::default());
            row[span.1..right].fill(Rgba8Pixel::default());
            *span = (left, right);
        }
        Image::from_rgba8(self.pixels.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(r: &Reveal, x: usize, y: usize) -> u8 {
        r.pixels.as_slice()[y * r.pixels.width() as usize + x].a
    }

    #[test]
    fn grows_from_the_origin() {
        let px = SharedPixelBuffer::<Rgba8Pixel>::new(100, 50);
        let mut px = px;
        px.make_mut_slice().fill(Rgba8Pixel { r: 1, g: 2, b: 3, a: 255 });
        let mut r = Reveal::new(px, Some((90.0, 10.0)));
        r.frame(0.0);
        assert_eq!(alpha(&r, 90, 10), 255);
        r.frame(0.2);
        assert_eq!(alpha(&r, 90, 10), 0, "the origin is uncovered first");
        assert_eq!(alpha(&r, 5, 45), 255, "the far corner stays old");
        r.frame(1.0);
        assert!(r.pixels.as_slice().iter().all(|p| p.a == 0), "everything is new at the end");
    }
}

//! Raster alpha mask: rounded clipping is unsupported by Slint's software renderer.
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub(crate) struct Reveal {
    pixels: SharedPixelBuffer<Rgba8Pixel>,
    spans: Vec<(usize, usize)>,
    radius: f32,
}

impl Reveal {
    pub fn new(pixels: SharedPixelBuffer<Rgba8Pixel>) -> Self {
        let width = pixels.width() as usize;
        let height = pixels.height() as usize;
        Self { pixels, spans: vec![(0, width); height], radius: (width as f32).hypot(height as f32) / 2.0 + 1.0 }
    }

    /// Progress is monotonic. Only the newly uncovered pixels are cleared.
    pub fn frame(&mut self, progress: f32) -> Image {
        let width = self.pixels.width() as usize;
        let cx = width as f32 / 2.0;
        let cy = self.pixels.height() as f32 / 2.0;
        let radius = self.radius * (1.0 - progress.clamp(0.0, 1.0));
        let pixels = self.pixels.make_mut_slice();
        for (y, span) in self.spans.iter_mut().enumerate() {
            let dy = y as f32 + 0.5 - cy;
            let half = (radius * radius - dy * dy).max(0.0).sqrt();
            let left = ((cx - half).floor() as usize).clamp(span.0, span.1);
            let right = if half == 0.0 { left } else { ((cx + half).ceil() as usize).clamp(left, span.1) };
            let row = &mut pixels[y * width..(y + 1) * width];
            row[span.0..left].fill(Rgba8Pixel::default());
            row[right..span.1].fill(Rgba8Pixel::default());
            *span = (left, right);
        }
        Image::from_rgba8(self.pixels.clone())
    }
}

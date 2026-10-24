//! Contact sheet of the ready-made backgrounds and plate textures:
//! `cargo run -p lb-gfx --example backdrops -- <out.png>`.

use lb_gfx::backdrops::{plates, stills};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "target/backdrops.png".into());
    let (tw, th) = (320u32, 180u32);
    let cols = 4;
    let rows_s = stills::STILLS.len().div_ceil(cols) as u32;
    let rows_p = plates::PLATES.len().div_ceil(cols) as u32;
    let ph = 64u32;
    let mut sheet = Pixmap::new(cols as u32 * tw, rows_s * th + rows_p * ph).unwrap();
    sheet.fill(tiny_skia::Color::from_rgba8(128, 128, 128, 255));
    for (i, b) in stills::STILLS.iter().enumerate() {
        let p = stills::paint(b.stem, tw, th).unwrap();
        sheet.draw_pixmap((i % cols) as i32 * tw as i32, (i / cols) as i32 * th as i32, p.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
    for (i, b) in plates::PLATES.iter().enumerate() {
        let p = plates::paint(b.stem).unwrap();
        let s = (tw - 8) as f32 / p.width() as f32;
        let t = Transform::from_row(s, 0.0, 0.0, s, ((i % cols) as u32 * tw + 4) as f32, (rows_s * th + (i / cols) as u32 * ph + 4) as f32);
        sheet.draw_pixmap(0, 0, p.as_ref(), &PixmapPaint::default(), t, None);
    }
    sheet.save_png(&out).unwrap();
    println!("{out}");
}

//! `laurabrod.svg` → `laurabrod.ico` (PNG-compressed images, 16…256 px).

use resvg::{tiny_skia, usvg};

const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

pub fn run() -> Result<(), String> {
    let svg_path = crate::app_assets().join("laurabrod.svg");
    let svg = std::fs::read(&svg_path).map_err(|e| format!("{}: {e}", svg_path.display()))?;
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).map_err(|e| e.to_string())?;

    let mut images = Vec::new();
    for size in SIZES {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("pixmap")?;
        let scale = size as f32 / tree.size().width();
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        images.push((size, pixmap.encode_png().map_err(|e| e.to_string())?));
    }

    let ico = encode_ico(&images);
    let out = crate::app_assets().join("laurabrod.ico");
    std::fs::write(&out, &ico).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("{} ({} KB, {} sizes)", out.display(), ico.len() / 1024, SIZES.len());
    Ok(())
}

/// ICONDIR + ICONDIRENTRY[] + PNG data.
fn encode_ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, png) in images {
        // 256 is stored as 0
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((png.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in images {
        out.extend(png);
    }
    out
}

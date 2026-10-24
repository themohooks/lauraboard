//! Avatar of a channel: a square picture (`channels/<id>/avatar.png`,
//! 256×256) shown by the interface next to the channel name, in the
//! channel list and in notifications; the graphics can show it too.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// File name inside the channel folder.
pub const FILE: &str = "avatar.png";
/// Side of the stored picture, pixels.
pub const SIZE: u32 = 256;
/// Largest accepted upload.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

pub fn path(channel_dir: &Path) -> PathBuf {
    channel_dir.join(FILE)
}

/// Version of the avatar for caches (its modification time, ms), `None`
/// without one.
pub fn version(channel_dir: &Path) -> Option<u64> {
    let m = std::fs::metadata(path(channel_dir)).ok()?;
    let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(t.as_millis() as u64)
}

/// The picture in `bytes` (PNG, JPEG, WebP…) cut to a centred square and
/// scaled to [`SIZE`], as PNG.
pub fn prepare(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("picture is larger than {} MB", MAX_BYTES / 1024 / 1024));
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("not a picture: {e}"))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err("empty picture".into());
    }
    let side = w.min(h);
    let square = img.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let out = square.resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut png = Vec::new();
    out.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(png)
}

/// Store a new avatar for the channel in `channel_dir`.
pub fn save(channel_dir: &Path, bytes: &[u8]) -> Result<(), String> {
    let png = prepare(bytes)?;
    let dst = path(channel_dir);
    let tmp = channel_dir.join("avatar.png.part");
    std::fs::write(&tmp, &png).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &dst).map_err(|e| format!("{}: {e}", dst.display()))
}

/// Remove the avatar (no error when there is none).
pub fn remove(channel_dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(path(channel_dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_to_a_centred_square() {
        // 300×100: red in the middle third, blue at the sides
        let mut img = image::RgbaImage::from_pixel(300, 100, image::Rgba([0, 0, 255, 255]));
        for x in 100..200 {
            for y in 0..100 {
                img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let mut src = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut src), image::ImageFormat::Png).unwrap();
        let out = image::load_from_memory(&prepare(&src).unwrap()).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (SIZE, SIZE));
        assert_eq!(out.get_pixel(5, 5)[0], 255, "the square is the red middle");
        assert!(prepare(b"not an image").is_err());
    }

    #[test]
    fn save_version_remove() {
        let dir = tempfile::tempdir().unwrap();
        assert!(version(dir.path()).is_none());
        let mut src = Vec::new();
        image::RgbaImage::new(10, 10).write_to(&mut std::io::Cursor::new(&mut src), image::ImageFormat::Png).unwrap();
        save(dir.path(), &src).unwrap();
        assert!(version(dir.path()).is_some());
        remove(dir.path()).unwrap();
        remove(dir.path()).unwrap();
        assert!(version(dir.path()).is_none());
    }
}

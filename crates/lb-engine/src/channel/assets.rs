//! Files the channel's screen graphics and settings point at (pictures,
//! SWF animations, font files, the standby image) are copied into
//! `channels/<id>/graphics/files/` when the document is saved, and the
//! document is changed to the copy: the channel no longer depends on the
//! originals and its folder holds everything it shows. Copies are named by
//! content, so saving again does not copy the same file twice.
//!
//! HTML animations stay where they are: a page loads its scripts and
//! pictures from beside it, a lone copy would break it.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use lb_core::config::ChannelConfig;
use lb_core::graphics::{ElementKind, GraphicsConfig};
use lb_core::media::is_url;
use lb_core::project::paths;

/// Sub-folder of the channel directory.
pub const FILES_DIR: &str = "graphics/files";
/// Larger files are left in place (pictures and fonts are far smaller).
const MAX_BYTES: u64 = 512 * 1024 * 1024;

/// Copy what `g` uses into the channel folder `dir` (paths stored relative
/// to the project `root`).
pub fn adopt_graphics(root: &Path, dir: &Path, g: &mut GraphicsConfig) {
    for e in &mut g.elements {
        if let ElementKind::Image { path } = &mut e.kind {
            adopt(root, dir, path);
        }
        if let ElementKind::Plate { icon: Some(icon), .. } = &mut e.kind {
            adopt(root, dir, &mut icon.path);
        }
        if let Some(image) = e.kind.shape_mut().and_then(|s| s.image.as_mut()) {
            adopt(root, dir, image);
        }
        for style in e.kind.styles_mut() {
            if let Some(font) = style.font.as_mut().filter(|f| is_font_file(f)) {
                adopt(root, dir, font);
            }
        }
    }
}

/// Copy the standby image of the channel settings.
pub fn adopt_config(root: &Path, dir: &Path, c: &mut ChannelConfig) {
    if let Some(image) = c.fallback.image.as_mut() {
        adopt(root, dir, image);
    }
}

/// A font given as a file (not a family name).
fn is_font_file(font: &str) -> bool {
    let lower = font.to_ascii_lowercase();
    [".ttf", ".otf", ".ttc"].iter().any(|e| lower.ends_with(e))
}

/// Replace `stored` with a copy inside the channel folder; anything that
/// cannot be copied (missing file, URL, page) stays as it is.
fn adopt(root: &Path, dir: &Path, stored: &mut String) {
    let s = stored.trim();
    if s.is_empty() || is_url(s) || s.contains('{') {
        return;
    }
    let lower = s.to_ascii_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        return;
    }
    let src = paths::resolve(root, s);
    match copy_in(&src, dir) {
        Ok(Some(copy)) => *stored = paths::to_stored(root, &copy),
        Ok(None) => {}
        Err(e) => tracing::warn!(file = %src.display(), "graphics file not copied into the channel: {e}"),
    }
}

/// `Ok(None)`: already inside the channel folder or not a usable file.
fn copy_in(src: &Path, dir: &Path) -> Result<Option<PathBuf>, String> {
    let Ok(meta) = std::fs::metadata(src) else { return Ok(None) };
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Ok(None);
    }
    let src_abs = std::fs::canonicalize(src).map_err(|e| e.to_string())?;
    let dir_abs = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if src_abs.starts_with(&dir_abs) {
        return Ok(None);
    }
    let bytes = std::fs::read(&src_abs).map_err(|e| e.to_string())?;
    let hash: String = Sha256::digest(&bytes).iter().take(8).map(|b| format!("{b:02x}")).collect();
    let stem: String = src
        .file_stem()
        .map(|s| s.to_string_lossy().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(60).collect())
        .unwrap_or_default();
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase())).unwrap_or_default();
    let files = dir.join(FILES_DIR);
    std::fs::create_dir_all(&files).map_err(|e| format!("{}: {e}", files.display()))?;
    let dest = files.join(format!("{stem}-{hash}{ext}"));
    if !dest.is_file() {
        // write beside and rename: a half-written file never gets the name
        let tmp = files.join(format!(".{stem}-{hash}{ext}.part"));
        std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(Some(dest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::graphics::Element;

    fn image(path: &str) -> Element {
        serde_json::from_value(serde_json::json!({"id": "logo", "type": "image", "path": path})).unwrap()
    }

    #[test]
    fn copies_outside_files_into_the_channel() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let dir = root.join("channels/main");
        std::fs::create_dir_all(&dir).unwrap();
        let outside = tmp.path().join("Мой логотип.png");
        std::fs::write(&outside, b"png").unwrap();

        let mut g = GraphicsConfig { elements: vec![image(&outside.to_string_lossy()), image("missing.png")], ..Default::default() };
        adopt_graphics(&root, &dir, &mut g);
        let ElementKind::Image { path } = &g.elements[0].kind else { panic!() };
        assert!(path.starts_with("channels/main/graphics/files/"), "{path}");
        assert!(path.ends_with(".png"));
        assert_eq!(std::fs::read(root.join(path)).unwrap(), b"png");
        assert_eq!(g.elements[1].kind, ElementKind::Image { path: "missing.png".into() }, "missing files stay");

        // saving again keeps the copy (no second copy, path unchanged)
        let before = g.clone();
        adopt_graphics(&root, &dir, &mut g);
        assert_eq!(g, before);
        assert_eq!(std::fs::read_dir(dir.join(FILES_DIR)).unwrap().count(), 1);
    }

    #[test]
    fn leaves_links_and_pages() {
        let tmp = tempfile::tempdir().unwrap();
        let page = tmp.path().join("anim.html");
        std::fs::write(&page, b"<html>").unwrap();
        for p in ["https://example.com/a.png", &page.to_string_lossy(), "{logo}", ""] {
            let mut s = p.to_string();
            adopt(tmp.path(), tmp.path(), &mut s);
            assert_eq!(s, p);
        }
        assert!(is_font_file("C:/Fonts/Inter.TTF"));
        assert!(!is_font_file("Inter"));
    }
}

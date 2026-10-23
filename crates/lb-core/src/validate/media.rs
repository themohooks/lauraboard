//! Media availability: files exist, are non-empty, probing succeeded.

use std::path::Path;

use super::Report;
use crate::graphics::ElementKind;
use crate::project::{ChannelData, paths};

/// Check that media files referenced by the library and graphics exist.
pub fn check_media_files(ch: &ChannelData, root: &Path, r: &mut Report) {
    for m in ch.library.items.iter().filter(|m| m.enabled) {
        let loc = format!("library/{}", m.id);
        if m.is_url() {
            continue;
        }
        let p = paths::resolve(root, &m.path);
        match std::fs::metadata(&p) {
            Ok(md) if md.len() == 0 => r.error(&loc, format!("file is empty: {}", p.display())),
            Ok(_) => {}
            Err(_) => r.error(&loc, format!("file not found: {}", p.display())),
        }
        if let Some(e) = &m.probe.error {
            r.error(&loc, format!("file could not be decoded: {e}"));
        }
    }
    for e in &ch.graphics.elements {
        if let ElementKind::Image { path } = &e.kind
            && !paths::resolve(root, path).exists()
        {
            r.error(format!("graphics/{}", e.id), format!("image not found: {path}"));
        }
        if let Some(img) = e.kind.shape().and_then(|s| s.image.as_ref())
            && !paths::resolve(root, img).exists()
        {
            r.warn(format!("graphics/{}", e.id), format!("image not found: {img}"));
        }
    }
    if let Some(img) = &ch.config.fallback.image
        && !paths::resolve(root, img).exists()
    {
        r.warn("channel.toml", format!("fallback image not found: {img}"));
    }
}

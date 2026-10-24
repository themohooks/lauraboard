//! Media availability: files exist, are non-empty, probing succeeded.

use std::path::Path;

use super::Report;
use crate::graphics::ElementKind;
use crate::project::{ChannelData, paths};

/// Check that media files referenced by the library and graphics exist.
pub fn check_media_files(ch: &ChannelData, root: &Path, r: &mut Report) {
    for m in ch.library.items.iter().filter(|m| m.enabled) {
        let loc = format!("library/{}", m.id);
        if let Some(scene) = &m.scene {
            check_scene(ch, &loc, scene, r);
            continue;
        }
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

/// A scene's background and music exist in the library.
fn check_scene(ch: &ChannelData, loc: &str, scene: &crate::media::Scene, r: &mut Report) {
    use crate::media::scene::Background;
    if let Background::Media { media } = &scene.background {
        match ch.library.get(media) {
            None => r.error(loc, format!("scene background '{media}' is not in the library")),
            Some(b) if b.is_scene() => r.error(loc, format!("scene background '{media}' is a scene itself")),
            Some(_) => {}
        }
    }
    for (i, rule) in scene.music.iter().enumerate().filter(|(_, m)| m.enabled) {
        for t in rule.tracks.iter().filter(|t| !ch.library.contains(t)) {
            r.warn(loc, format!("music rule {}: track '{t}' is not in the library", i + 1));
        }
        if rule.pool(&ch.library).is_empty() {
            r.warn(loc, format!("music rule {}: no tracks to play", i + 1));
        }
    }
}

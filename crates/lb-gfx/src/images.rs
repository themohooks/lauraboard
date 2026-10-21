//! Image loading into premultiplied pixmaps, cached per (path, size).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tiny_skia::{IntSize, Pixmap, PixmapPaint, Transform};

pub struct ImageCache {
    root: PathBuf,
    originals: HashMap<String, Option<Arc<Pixmap>>>,
    scaled: HashMap<(String, u32, u32), Arc<Pixmap>>,
}

impl ImageCache {
    pub fn new(root: &Path) -> Self {
        ImageCache { root: root.to_path_buf(), originals: HashMap::new(), scaled: HashMap::new() }
    }

    /// Original image (cached; failures are cached too and logged once).
    pub fn original(&mut self, path: &str) -> Option<Arc<Pixmap>> {
        if let Some(p) = self.originals.get(path) {
            return p.clone();
        }
        let full = lb_core::project::paths::resolve(&self.root, path);
        let loaded = load(&full);
        if loaded.is_none() {
            tracing::warn!(path = %full.display(), "graphics image could not be loaded");
        }
        let v = loaded.map(Arc::new);
        self.originals.insert(path.to_string(), v.clone());
        v
    }

    /// Image scaled to (w, h).
    pub fn scaled(&mut self, path: &str, w: u32, h: u32) -> Option<Arc<Pixmap>> {
        let orig = self.original(path)?;
        if orig.width() == w && orig.height() == h {
            return Some(orig);
        }
        let key = (path.to_string(), w, h);
        if let Some(p) = self.scaled.get(&key) {
            return Some(p.clone());
        }
        let mut pm = Pixmap::new(w.max(1), h.max(1))?;
        let t = Transform::from_scale(w as f32 / orig.width() as f32, h as f32 / orig.height() as f32);
        let paint = PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
        pm.draw_pixmap(0, 0, Pixmap::as_ref(&orig), &paint, t, None);
        let pm = Arc::new(pm);
        self.scaled.insert(key, pm.clone());
        Some(pm)
    }

    /// Forget everything (e.g. after graphics configuration changes).
    pub fn clear(&mut self) {
        self.originals.clear();
        self.scaled.clear();
    }
}

fn load(path: &Path) -> Option<Pixmap> {
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    // premultiply
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u16;
        if a < 255 {
            px[0] = ((px[0] as u16 * a + 127) / 255) as u8;
            px[1] = ((px[1] as u16 * a + 127) / 255) as u8;
            px[2] = ((px[2] as u16 * a + 127) / 255) as u8;
        }
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h)?)
}

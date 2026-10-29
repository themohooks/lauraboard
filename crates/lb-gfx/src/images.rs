//! Image loading into premultiplied pixmaps, cached per (path, size).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tiny_skia::{IntSize, Pixmap, PixmapPaint, Transform};

pub struct ImageCache {
    root: PathBuf,
    web_cursors: std::cell::RefCell<HashMap<usize, u64>>,
    web_active: std::cell::RefCell<Vec<Arc<crate::web::WebSource>>>,
    originals: HashMap<String, Option<Arc<Pixmap>>>,
    scaled: HashMap<(String, u32, u32), Arc<Pixmap>>,
}

impl ImageCache {
    pub fn web(&self, path: &str, w: u32, h: u32) -> Arc<crate::web::WebSource> {
        let source = crate::web::WebSource::get(&lb_core::project::paths::resolve(&self.root, path), w, h);
        let mut active = self.web_active.borrow_mut();
        if !active.iter().any(|s| Arc::ptr_eq(s, &source)) {
            active.push(source.clone());
        }
        source
    }
    pub fn new(root: &Path) -> Self {
        ImageCache {
            web_cursors: Default::default(),
            web_active: Default::default(),
            root: root.to_path_buf(),
            originals: HashMap::new(),
            scaled: HashMap::new(),
        }
    }

    pub fn begin_frame(&self) {
        self.web_active.borrow_mut().clear();
    }
    pub fn mix_audio(&self, out: &mut [f32]) {
        let active = self.web_active.borrow();
        let mut cursors = self.web_cursors.borrow_mut();
        cursors.retain(|key, _| active.iter().any(|s| Arc::as_ptr(s) as usize == *key));
        for source in active.iter() {
            source.mix_audio(cursors.entry(Arc::as_ptr(source) as usize).or_default(), out);
        }
    }
    /// Snapshot for the live audio pump; mixing must not wait for a render.
    pub fn audio_sources(&self) -> Vec<Arc<crate::web::WebSource>> {
        self.web_active.borrow().clone()
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

    pub fn invalidate(&mut self, path: &str, original: bool) {
        if original {
            self.originals.remove(path);
        }
        self.scaled.retain(|(p, _, _), _| p != path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_colour_of_semi_transparent_png() {
        let dir = std::env::temp_dir().join(format!("lbgfx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("red.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0xC0, 0x18, 0x2B, 0xCC])).save(&p).unwrap();
        let mut c = ImageCache::new(&dir);
        let pm = c.scaled("red.png", 4, 4).unwrap();
        let px = pm.pixel(1, 1).unwrap().demultiply();
        assert!(px.red().abs_diff(0xC0) <= 1 && px.green().abs_diff(0x18) <= 1 && px.alpha() == 0xCC, "{px:?}");
        let pm2 = c.scaled("red.png", 2, 2).unwrap();
        let px2 = pm2.pixel(1, 1).unwrap().demultiply();
        assert!(px2.red() > 0xA0 && px2.green() < 0x40, "{px2:?}");
    }
}

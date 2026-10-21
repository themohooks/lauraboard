//! Font lookup: a family name from the system font database, a path to a
//! font file, or the default sans-serif fallback.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ab_glyph::FontArc;

pub struct FontStore {
    db: fontdb::Database,
    cache: HashMap<String, Option<FontArc>>,
    root: PathBuf,
    default: Option<FontArc>,
}

const FALLBACK_FAMILIES: &[&str] = &["Arial", "Segoe UI", "DejaVu Sans", "Liberation Sans", "Noto Sans", "Roboto", "Helvetica"];

impl FontStore {
    /// `root` resolves relative font paths (project root).
    pub fn new(root: &Path) -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut s = FontStore { db, cache: HashMap::new(), root: root.to_path_buf(), default: None };
        s.default = FALLBACK_FAMILIES.iter().find_map(|f| s.load_family(f)).or_else(|| s.first_any());
        if s.default.is_none() {
            tracing::warn!("no usable system fonts found; text graphics will not render");
        }
        s
    }

    /// Font by name or path; falls back to the default font.
    pub fn get(&mut self, spec: Option<&str>) -> Option<FontArc> {
        let Some(spec) = spec.filter(|s| !s.trim().is_empty()) else { return self.default.clone() };
        if let Some(f) = self.cache.get(spec) {
            return f.clone().or_else(|| self.default.clone());
        }
        let font = if looks_like_path(spec) { self.load_file(spec) } else { self.load_family(spec) };
        if font.is_none() {
            tracing::warn!(font = spec, "font not found, using default");
        }
        self.cache.insert(spec.to_string(), font.clone());
        font.or_else(|| self.default.clone())
    }

    fn load_file(&self, spec: &str) -> Option<FontArc> {
        let p = lb_core::project::paths::resolve(&self.root, spec);
        let data = std::fs::read(p).ok()?;
        FontArc::try_from_vec(data).ok()
    }

    fn load_family(&self, family: &str) -> Option<FontArc> {
        let q = fontdb::Query { families: &[fontdb::Family::Name(family)], ..Default::default() };
        let id = self.db.query(&q)?;
        self.face_to_font(id)
    }

    fn first_any(&self) -> Option<FontArc> {
        self.db.faces().find_map(|f| self.face_to_font(f.id))
    }

    fn face_to_font(&self, id: fontdb::ID) -> Option<FontArc> {
        self.db
            .with_face_data(id, |data, index| ab_glyph::FontVec::try_from_vec_and_index(data.to_vec(), index).ok().map(FontArc::new))
            .flatten()
    }
}

fn looks_like_path(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.ends_with(".ttf") || l.ends_with(".otf") || l.ends_with(".ttc") || s.contains('/') || s.contains('\\')
}

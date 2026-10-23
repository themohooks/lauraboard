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
        self.get_styled(spec, false, false)
    }

    /// Font by name or path in a weight / slant. A family without a bold
    /// or italic face gives its regular one; font files are used as they are.
    pub fn get_styled(&mut self, spec: Option<&str>, bold: bool, italic: bool) -> Option<FontArc> {
        let spec = spec.map(str::trim).filter(|s| !s.is_empty());
        if spec.is_none() && !bold && !italic {
            return self.default.clone();
        }
        let key = format!("{}|{bold}|{italic}", spec.unwrap_or(""));
        if let Some(f) = self.cache.get(&key) {
            return f.clone().or_else(|| self.default.clone());
        }
        let font = match spec {
            Some(s) if looks_like_path(s) => self.load_file(s),
            Some(s) => self.load_styled(s, bold, italic),
            None => FALLBACK_FAMILIES.iter().find_map(|f| self.load_styled(f, bold, italic)),
        };
        if font.is_none() && let Some(s) = spec {
            tracing::warn!(font = s, "font not found, using default");
        }
        self.cache.insert(key, font.clone());
        font.or_else(|| self.default.clone())
    }

    /// Families of the system fonts, sorted.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.db.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup();
        v
    }

    fn load_styled(&self, family: &str, bold: bool, italic: bool) -> Option<FontArc> {
        let q = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
            style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
            ..Default::default()
        };
        let id = self.db.query(&q)?;
        self.face_to_font(id)
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

/// The bytes are a font file that text can be drawn with.
pub fn is_usable(data: &[u8]) -> bool {
    FontArc::try_from_vec(data.to_vec()).is_ok()
}

fn looks_like_path(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.ends_with(".ttf") || l.ends_with(".otf") || l.ends_with(".ttc") || s.contains('/') || s.contains('\\')
}

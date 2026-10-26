//! Google Fonts: the family catalogue and downloading a family (regular,
//! bold, italic, bold italic as static TrueType files) into a channel's
//! font folder, where the graphics find it by its family name.
//!
//! No API key is needed: the catalogue is the public metadata of
//! fonts.google.com, the files come from the CSS API, which serves
//! TrueType to clients that are not browsers.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

const CATALOG_URL: &str = "https://fonts.google.com/metadata/fonts";
const CSS_URL: &str = "https://fonts.googleapis.com/css2";
/// The catalogue changes rarely.
const CATALOG_TTL: Duration = Duration::from_secs(24 * 3600);
const MAX_FONT_BYTES: u64 = 40 * 1024 * 1024;
/// Sub-folder of the channel font folder.
pub const DIR: &str = "google";

#[derive(Debug, Clone, Serialize)]
pub struct Family {
    pub family: String,
    /// "Sans Serif", "Serif", "Display", "Handwriting", "Monospace".
    pub category: String,
    /// Has Cyrillic letters.
    pub cyrillic: bool,
    pub bold: bool,
    pub italic: bool,
    pub popularity: i64,
}

static CATALOG: Mutex<Option<(Instant, Vec<Family>)>> = Mutex::new(None);

fn agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).build();
    ureq::Agent::new_with_config(cfg)
}

fn get_text(url: &str) -> Result<String, String> {
    // a plain user agent: the CSS API then answers with TrueType URLs
    agent()
        .get(url)
        .header("User-Agent", "LauraBoard")
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))
}

/// Parse the fonts.google.com metadata (it may start with `)]}'`).
pub fn parse_catalog(text: &str) -> Result<Vec<Family>, String> {
    let text = text.trim_start_matches(")]}'").trim_start();
    let v: Value = serde_json::from_str(text).map_err(|e| format!("Google Fonts catalogue: {e}"))?;
    let list = v["familyMetadataList"].as_array().ok_or("Google Fonts catalogue: no family list")?;
    let mut out: Vec<Family> = list
        .iter()
        .filter_map(|f| {
            let family = f["family"].as_str()?.to_string();
            let styles: Vec<&str> = f["fonts"].as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
            let subsets = f["subsets"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default();
            Some(Family {
                family,
                category: f["category"].as_str().unwrap_or_default().to_string(),
                cyrillic: subsets.contains(&"cyrillic"),
                bold: styles.iter().any(|s| s.starts_with("700")),
                italic: styles.iter().any(|s| s.ends_with('i')),
                popularity: f["popularity"].as_i64().unwrap_or(i64::MAX),
            })
        })
        .collect();
    // most popular first (1 is the most popular)
    out.sort_by_key(|f| f.popularity);
    Ok(out)
}

/// The whole catalogue (downloaded once a day).
pub fn catalog() -> Result<Vec<Family>, String> {
    if let Ok(g) = CATALOG.lock()
        && let Some((at, list)) = g.as_ref()
        && at.elapsed() < CATALOG_TTL
    {
        return Ok(list.clone());
    }
    let list = parse_catalog(&get_text(CATALOG_URL)?)?;
    if let Ok(mut g) = CATALOG.lock() {
        *g = Some((Instant::now(), list.clone()));
    }
    Ok(list)
}

/// Families whose name contains `q` (case-insensitive), at most `limit`.
pub fn search(q: &str, cyrillic_only: bool, limit: usize) -> Result<Vec<Family>, String> {
    let q = q.trim().to_lowercase();
    Ok(catalog()?
        .into_iter()
        .filter(|f| (!cyrillic_only || f.cyrillic) && (q.is_empty() || f.family.to_lowercase().contains(&q)))
        .take(limit)
        .collect())
}

/// `(style, url)` of the TrueType files in a CSS API answer; style is
/// "Regular", "Bold", "Italic" or "BoldItalic".
pub fn parse_css(css: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in css.split("@font-face").skip(1) {
        let prop = |name: &str| {
            block.split(';').find_map(|l| {
                l.trim().trim_start_matches('{').trim().strip_prefix(name).map(|v| v.trim_start_matches(':').trim().to_string())
            })
        };
        let italic = prop("font-style").is_some_and(|s| s == "italic");
        let bold = prop("font-weight").and_then(|w| w.parse::<u32>().ok()).is_some_and(|w| w >= 600);
        let Some(url) = block.split("url(").nth(1).and_then(|r| r.split(')').next()) else { continue };
        let style = match (bold, italic) {
            (false, false) => "Regular",
            (true, false) => "Bold",
            (false, true) => "Italic",
            (true, true) => "BoldItalic",
        };
        if !out.iter().any(|(s, _)| s == style) {
            out.push((style.to_string(), url.trim_matches(['\'', '"']).to_string()));
        }
    }
    out
}

/// File-system safe version of a family name.
fn file_stem(family: &str) -> String {
    family.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

/// Download `family` into `<fonts_dir>/google/<Family>/`. Returns the
/// files written. The family keeps its Google name, so a text style uses
/// it as `font = "Roboto"`.
pub fn download(family: &str, fonts_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let family = family.trim();
    if family.is_empty() {
        return Err("font family is empty".into());
    }
    let info = catalog()?
        .into_iter()
        .find(|f| f.family.eq_ignore_ascii_case(family))
        .ok_or_else(|| format!("font '{family}' not found in Google Fonts"))?;
    let mut axes = vec!["0,400"];
    if info.bold {
        axes.push("0,700");
    }
    if info.italic {
        axes.push("1,400");
        if info.bold {
            axes.push("1,700");
        }
    }
    let spec = format!("{}:ital,wght@{}", info.family.replace(' ', "+"), axes.join(";"));
    let css = match get_text(&format!("{CSS_URL}?family={spec}")) {
        Ok(c) => c,
        // some families have no such axes: ask for the default style only
        Err(_) => get_text(&format!("{CSS_URL}?family={}", info.family.replace(' ', "+")))?,
    };
    let files = parse_css(&css);
    if files.is_empty() {
        return Err(format!("font '{}': Google Fonts gave no files", info.family));
    }
    let stem = file_stem(&info.family);
    let dir = fonts_dir.join(DIR).join(&stem);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut written = Vec::new();
    for (style, url) in files {
        let data = agent()
            .get(&url)
            .call()
            .map_err(|e| format!("{url}: {e}"))?
            .body_mut()
            .with_config()
            .limit(MAX_FONT_BYTES)
            .read_to_vec()
            .map_err(|e| format!("{url}: {e}"))?;
        if !lb_gfx::fonts::is_usable(&data) {
            return Err(format!("font '{}' ({style}): the downloaded file is not a usable font", info.family));
        }
        let path = dir.join(format!("{stem}-{style}.ttf"));
        std::fs::write(&path, &data).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_gives_styles_and_urls() {
        let css = "@font-face {\n  font-family: 'Roboto';\n  font-style: italic;\n  font-weight: 700;\n  src: url(https://x/a.ttf) format('truetype');\n}\n@font-face {\n  font-family: 'Roboto';\n  font-style: normal;\n  font-weight: 400;\n  src: url(https://x/b.ttf) format('truetype');\n}";
        let f = parse_css(css);
        assert_eq!(f, vec![("BoldItalic".into(), "https://x/a.ttf".into()), ("Regular".into(), "https://x/b.ttf".into())]);
    }

    #[test]
    fn catalog_parses_and_sorts_by_popularity() {
        let text = r#")]}'
{"familyMetadataList":[
 {"family":"B","category":"Serif","subsets":["latin"],"fonts":{"400":{}},"popularity":9},
 {"family":"A Sans","category":"Sans Serif","subsets":["latin","cyrillic"],"fonts":{"400":{},"700":{},"400i":{}},"popularity":2}
]}"#;
        let c = parse_catalog(text).unwrap();
        assert_eq!(c[0].family, "A Sans");
        assert!(c[0].cyrillic && c[0].bold && c[0].italic);
        assert!(!c[1].cyrillic && !c[1].bold);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_stem("PT Sans Narrow"), "PTSansNarrow");
        assert_eq!(file_stem("../x"), "x");
    }
}

//! Fonts for graphics: families installed on the server and font files of
//! the channel (`channels/<id>/fonts/`), plus adding a font file.

use std::path::PathBuf;
use std::sync::OnceLock;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use lb_core::config::Role;
use lb_core::project::paths;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

/// Folder of a channel holding its font files.
const FONTS_DIR: &str = "fonts";
const FONT_EXT: &[&str] = &["ttf", "otf", "ttc"];

/// System families are scanned once (it takes a moment).
fn system_families() -> &'static Vec<String> {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| lb_gfx::fonts::FontStore::new(std::path::Path::new(".")).families())
}

fn is_font(p: &std::path::Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| FONT_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

pub async fn list(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let dir = h.read(|d| d.dir.join(FONTS_DIR));
    let root = h.shared.root.clone();
    let (system, files) = tokio::task::spawn_blocking(move || {
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| is_font(p)).map(|p| paths::to_stored(&root, &p)).collect())
            .unwrap_or_default();
        files.sort();
        (system_families().clone(), files)
    })
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({ "system": system, "files": files })))
}

#[derive(Deserialize)]
pub struct AddFont {
    /// Font file on the server.
    pub path: String,
}

/// Copy a font file into the channel; returns the stored path to use in
/// a text style.
pub async fn add(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(b): Json<AddFont>) -> ApiResult<Json<Value>> {
    c.require(Role::Operator)?;
    let src = PathBuf::from(b.path.trim());
    if !is_font(&src) {
        return Err(ApiError::bad_request("not a font file (.ttf, .otf, .ttc)"));
    }
    let h = channel(&s, &id)?;
    let dir = h.read(|d| d.dir.join(FONTS_DIR));
    let root = h.shared.root.clone();
    let stored = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let data = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        if !lb_gfx::fonts::is_usable(&data) {
            return Err(format!("{}: not a usable font", src.display()));
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "font.ttf".into());
        let mut target = dir.join(&name);
        // a different file of the same name gets a number
        let mut n = 2;
        while target.exists() && std::fs::read(&target).ok().as_deref() != Some(&data[..]) {
            let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = std::path::Path::new(&name).extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            target = dir.join(format!("{stem}-{n}.{ext}"));
            n += 1;
        }
        std::fs::write(&target, &data).map_err(|e| format!("{}: {e}", target.display()))?;
        Ok(paths::to_stored(&root, &target))
    })
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?
    .map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "path": stored })))
}

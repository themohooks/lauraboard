//! What an installation created, so the uninstaller removes exactly that
//! and nothing else (`<config dir>/laurabrod/installation.json`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Receipt {
    pub version: String,
    pub dir: PathBuf,
    /// Unpacked files, relative to `dir`.
    pub files: Vec<String>,
    /// Shortcuts, menu entries, icons and links outside `dir`.
    pub extras: Vec<PathBuf>,
    /// Directory added to the user's PATH (Windows).
    pub path_entry: Option<PathBuf>,
    /// Listed in "Apps & features" (Windows).
    pub registered: bool,
}

/// `<config dir>/laurabrod` — shared with the application.
pub fn config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("laurabrod"))
}

fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("installation.json"))
}

impl Receipt {
    pub fn load() -> Option<Receipt> {
        let bytes = std::fs::read(path()?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self) -> Result<(), String> {
        write_json(&path().ok_or("нет каталога настроек")?, self)
    }

    pub fn delete() {
        if let Some(p) = path() {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let json = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
}

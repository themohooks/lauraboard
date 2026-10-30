//! What an installation created, so the uninstaller removes exactly that
//! and nothing else, and an update repeats the same choices:
//! `<config dir>/lauraboard/installation.json` for a per-user installation,
//! `<machine data dir>/installation.json` for one for all users.

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
    /// Directory added to the PATH (Windows).
    pub path_entry: Option<PathBuf>,
    /// Listed in "Apps & features" (Windows).
    pub registered: bool,
    /// Installed for every user of the computer.
    pub all_users: bool,
    /// Options chosen at installation (absent in receipts of 0.2 and older).
    pub menu_shortcut: Option<bool>,
    pub desktop_shortcut: Option<bool>,
    pub add_to_path: Option<bool>,
}

/// `<config dir>/lauraboard` — shared with the application.
pub fn config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("lauraboard"))
}

fn path(all_users: bool) -> Option<PathBuf> {
    let dir = if all_users { crate::platform::machine_data_dir()? } else { config_dir()? };
    Some(dir.join("installation.json"))
}

impl Receipt {
    /// The user's own installation first, else the one for all users.
    pub fn load() -> Option<Receipt> {
        [false, true].into_iter().find_map(|all| {
            let bytes = std::fs::read(path(all)?).ok()?;
            let r: Receipt = serde_json::from_slice(&bytes).ok()?;
            Some(Receipt { all_users: all, ..r })
        })
    }

    pub fn save(&self) -> Result<(), String> {
        write_json(&path(self.all_users).ok_or("нет каталога настроек")?, self)
    }

    pub fn delete(&self) {
        if let Some(p) = path(self.all_users) {
            let _ = std::fs::remove_file(&p);
            if self.all_users
                && let Some(dir) = p.parent()
            {
                let _ = std::fs::remove_dir(dir);
            }
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

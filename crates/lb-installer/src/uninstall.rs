//! Remove exactly what the receipt lists. Projects, media and anything else
//! the user put into the folder stay where they are.

use std::path::{Path, PathBuf};

use crate::install::Report;
use crate::payload;
use crate::platform;
use crate::receipt::{Receipt, config_dir};

/// Shortcuts, menu entries, PATH and the "Apps & features" entry.
pub fn remove_integration(r: &Receipt) {
    for p in &r.extras {
        let _ = std::fs::remove_file(p);
    }
    if let Some(entry) = &r.path_entry {
        let _ = platform::remove_from_path(entry);
    }
    if r.registered {
        platform::unregister();
    }
}

/// Delete `files` (relative to `dir`) and the folders they leave empty.
/// Returns the files that could not be deleted (in use).
pub fn remove_files(dir: &Path, files: &[String]) -> Vec<PathBuf> {
    let mut busy = Vec::new();
    for f in files {
        let path = dir.join(f);
        let _ = std::fs::remove_file(payload::old_name(&path));
        if std::fs::remove_file(&path).is_err() && path.exists() {
            busy.push(path);
        }
    }
    // deepest folders first
    let mut parents: Vec<PathBuf> = files.iter().filter_map(|f| dir.join(f).parent().map(Path::to_path_buf)).filter(|p| p != dir).collect();
    parents.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    parents.dedup();
    for p in parents {
        let _ = std::fs::remove_dir(p);
    }
    busy
}

pub fn uninstall(r: &Receipt, forget_settings: bool, report: Report) -> Result<Vec<String>, String> {
    report(0.1, "Удаление ярлыков");
    remove_integration(r);

    report(0.3, "Удаление файлов");
    let busy = remove_files(&r.dir, &r.files);
    Receipt::delete();
    if forget_settings && let Some(cfg) = config_dir() {
        for name in ["gui.json", "desktop.json"] {
            let _ = std::fs::remove_file(cfg.join(name));
        }
    }

    // the running uninstaller (and anything else still open) goes last
    let me = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
    let warnings = busy
        .iter()
        .filter(|p| p.canonicalize().ok() != me)
        .map(|p| format!("{} занят — закройте Laurabrod, файл будет удалён позже", p.display()))
        .collect();
    platform::delete_later(&busy, &r.dir);
    report(1.0, "Готово");
    Ok(warnings)
}

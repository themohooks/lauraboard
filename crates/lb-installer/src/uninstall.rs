//! Remove exactly what the receipt lists. Projects, media and anything else
//! the user put into the folder stay where they are — unless the user asks
//! to delete the channel data too ([`remove_project_data`]).

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

/// What a project keeps about channels and broadcasts: settings, playlists,
/// schedules, rules, graphics, runtime state, as-run logs, program logs and
/// generated HLS output. Media and recordings are the user's files and stay.
pub const PROJECT_DATA: [&str; 4] = ["laurabrod.toml", "channels", "logs", "output"];

/// Delete the channel / broadcast data of `project` (and the folder, if
/// nothing else is left in it). Returns warnings for what could not go.
pub fn remove_project_data(project: &Path) -> Vec<String> {
    // never touch a folder that is not a Laurabrod project
    if !project.join("laurabrod.toml").is_file() {
        return vec![format!("{}: не похоже на проект Laurabrod, данные не удалены", project.display())];
    }
    let mut warnings = Vec::new();
    for name in PROJECT_DATA {
        let p = project.join(name);
        let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else if p.exists() { std::fs::remove_file(&p) } else { Ok(()) };
        if let Err(e) = r {
            warnings.push(format!("{}: {e}", p.display()));
        }
    }
    let _ = std::fs::remove_dir(project);
    warnings
}

/// `remove_data`: project whose channel data is deleted as well.
pub fn uninstall(r: &Receipt, forget_settings: bool, remove_data: Option<&Path>, report: Report) -> Result<Vec<String>, String> {
    report(0.1, "Удаление ярлыков");
    remove_integration(r);

    let mut data_warnings = Vec::new();
    if let Some(project) = remove_data {
        report(0.2, "Удаление данных каналов и эфиров");
        data_warnings = remove_project_data(project);
    }

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
    let mut warnings: Vec<String> = busy
        .iter()
        .filter(|p| p.canonicalize().ok() != me)
        .map(|p| format!("{} занят — закройте Laurabrod, файл будет удалён позже", p.display()))
        .collect();
    warnings.extend(data_warnings);
    platform::delete_later(&busy, &r.dir);
    report(1.0, "Готово");
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lb-uninstall-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn project_data_goes_media_stays() {
        let p = temp("data");
        std::fs::write(p.join("laurabrod.toml"), "").unwrap();
        for d in ["channels/main/asrun", "logs", "media", "recordings"] {
            std::fs::create_dir_all(p.join(d)).unwrap();
        }
        std::fs::write(p.join("media/clip.mp4"), "x").unwrap();
        assert!(remove_project_data(&p).is_empty());
        assert!(!p.join("laurabrod.toml").exists() && !p.join("channels").exists() && !p.join("logs").exists());
        assert!(p.join("media/clip.mp4").exists() && p.join("recordings").exists());
        // a folder with only project data disappears completely
        let q = temp("empty");
        std::fs::write(q.join("laurabrod.toml"), "").unwrap();
        std::fs::create_dir_all(q.join("channels/main")).unwrap();
        assert!(remove_project_data(&q).is_empty());
        assert!(!q.exists());
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn other_folders_are_not_touched() {
        let p = temp("foreign");
        std::fs::create_dir_all(p.join("channels")).unwrap();
        assert_eq!(remove_project_data(&p).len(), 1);
        assert!(p.join("channels").exists());
        let _ = std::fs::remove_dir_all(&p);
    }
}

//! What to install and where (the choices of the wizard or the command line).

use std::ffi::OsString;
use std::path::PathBuf;

use crate::receipt::Receipt;

#[derive(Debug, Clone)]
pub struct Options {
    pub dir: PathBuf,
    /// For every user of the computer (needs administrator / root rights).
    pub all_users: bool,
    /// Start menu (Windows) / applications menu (Linux) entry.
    pub menu_shortcut: bool,
    pub desktop_shortcut: bool,
    /// `lauraboard` command in new terminals.
    pub add_to_path: bool,
    /// Create this project (if missing) and open it from shortcuts.
    pub project: Option<PathBuf>,
    /// Fill a new project with the demo channel (needs FFmpeg).
    pub demo: bool,
}

impl Options {
    /// Defaults; an update keeps the folder, the scope and the integration
    /// chosen before, and leaves projects alone.
    pub fn new(existing: Option<&Receipt>) -> Self {
        let kept = |pick: fn(&Receipt) -> Option<bool>| existing.and_then(pick).unwrap_or(true);
        let all_users = existing.is_some_and(|r| r.all_users);
        Options {
            dir: existing.map(|r| r.dir.clone()).unwrap_or_else(|| default_dir(all_users)),
            all_users,
            menu_shortcut: kept(|r| r.menu_shortcut),
            desktop_shortcut: kept(|r| r.desktop_shortcut),
            add_to_path: kept(|r| r.add_to_path),
            project: existing.is_none().then(default_project),
            demo: false,
        }
    }
}

impl Options {
    /// The same choices as command-line options, for the copy started with
    /// administrator rights (`--resume` / `--silent`).
    pub fn to_args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec!["--dir".into(), self.dir.clone().into()];
        let flags = [
            (self.all_users, "--all-users"),
            (!self.menu_shortcut, "--no-menu"),
            (!self.desktop_shortcut, "--no-desktop"),
            (!self.add_to_path, "--no-path"),
            (self.project.is_none(), "--no-project"),
            (self.demo, "--demo"),
        ];
        a.extend(flags.into_iter().filter(|(on, _)| *on).map(|(_, f)| f.into()));
        if let Some(p) = &self.project {
            a.extend(["--project".into(), p.clone().into()]);
        }
        a
    }
}

/// Per-user location, no administrator rights needed:
/// `%LOCALAPPDATA%\Programs\LauraBoard` or `~/.local/share/lauraboard`;
/// for all users `%ProgramFiles%\LauraBoard` or `/opt/lauraboard`.
pub fn default_dir(all_users: bool) -> PathBuf {
    let base = crate::platform::programs_dir(all_users).unwrap_or_else(|| PathBuf::from("."));
    base.join(if cfg!(windows) { "LauraBoard" } else { "lauraboard" })
}

/// Same default as the application uses: `<Documents>/LauraBoard`.
pub fn default_project() -> PathBuf {
    dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("LauraBoard")
}

/// A folder picked in a dialog gets its own `LauraBoard` subfolder, so the
/// files never spill into e.g. `C:\Program Files` itself.
pub fn with_app_folder(picked: PathBuf) -> PathBuf {
    let named = picked.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("lauraboard"));
    if named { picked } else { picked.join("LauraBoard") }
}

/// Check an install folder typed by the user: `Ok(note)` or `Err(problem)`.
pub fn check_dir(text: &str, binary: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Укажите папку".into());
    }
    let path = std::path::Path::new(text);
    if !path.is_absolute() {
        return Err("Нужен полный путь".into());
    }
    if path.is_file() {
        return Err("Это файл, а не папка".into());
    }
    if !path.exists() {
        return Ok("Папка будет создана".into());
    }
    if path.join(binary).exists() {
        return Ok("Здесь уже установлен LauraBoard — файлы будут заменены".into());
    }
    let empty = std::fs::read_dir(path).map(|mut d| d.next().is_none()).unwrap_or(true);
    Ok(if empty {
        String::new()
    } else {
        "Папка не пуста — файлы программы добавятся к имеющимся".into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_folder_gets_app_subfolder() {
        let base = std::env::temp_dir();
        assert_eq!(with_app_folder(base.clone()), base.join("LauraBoard"));
        assert_eq!(with_app_folder(base.join("lauraboard")), base.join("lauraboard"));
    }

    #[test]
    fn folder_checks() {
        assert!(check_dir("", "x").is_err());
        assert!(check_dir("relative/dir", "x").is_err());
        let tmp = std::env::temp_dir().join(format!("lb-setup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(check_dir(tmp.to_str().unwrap(), "app").unwrap(), "Папка будет создана");
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(check_dir(tmp.to_str().unwrap(), "app").unwrap(), "");
        std::fs::write(tmp.join("other.txt"), "x").unwrap();
        assert!(check_dir(tmp.to_str().unwrap(), "app").unwrap().contains("не пуста"));
        std::fs::write(tmp.join("app"), "x").unwrap();
        assert!(check_dir(tmp.to_str().unwrap(), "app").unwrap().contains("уже установлен"));
        assert!(check_dir(tmp.join("app").to_str().unwrap(), "app").is_err());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}

//! What to install and where (the choices of the wizard or the command line).

use std::path::PathBuf;

use crate::receipt::Receipt;

#[derive(Debug, Clone)]
pub struct Options {
    pub dir: PathBuf,
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
    /// Defaults; an update keeps the directory and leaves projects alone.
    pub fn new(existing: Option<&Receipt>) -> Self {
        Options {
            dir: existing.map(|r| r.dir.clone()).unwrap_or_else(default_dir),
            menu_shortcut: true,
            desktop_shortcut: true,
            add_to_path: true,
            project: existing.is_none().then(default_project),
            demo: false,
        }
    }
}

/// Per-user location, no administrator rights needed:
/// `%LOCALAPPDATA%\Programs\LauraBoard` or `~/.local/share/lauraboard`.
pub fn default_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(windows) { base.join("Programs").join("LauraBoard") } else { base.join("lauraboard") }
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

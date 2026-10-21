//! Linux (and other Unix desktops): freedesktop menu entry and icon,
//! desktop launcher, `~/.local/bin` link — all inside the home directory.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{APP_COMMENT, APP_NAME, PathChange, Registration};

pub fn no_window(_: &mut Command) {}

fn data_dir() -> Result<PathBuf, String> {
    dirs::data_local_dir().ok_or_else(|| "нет каталога ~/.local/share".to_string())
}

fn desktop_entry(binary: &Path, icon: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={APP_NAME}\nGenericName=TV playout\nComment={APP_COMMENT}\n\
         Exec=\"{}\"\nPath={}\nIcon={icon}\nTerminal=false\nCategories=AudioVideo;Video;\nKeywords=tv;broadcast;playout;stream;\n",
        binary.display(),
        binary.parent().unwrap_or(Path::new("/")).display()
    )
}

fn write(path: &Path, text: &str, executable: bool) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    if executable {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

fn quiet(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// Applications menu: `~/.local/share/applications/laurabrod.desktop` plus
/// the icon in the hicolor theme.
pub fn menu_shortcut(binary: &Path, icon: &Path) -> Result<Vec<PathBuf>, String> {
    let data = data_dir()?;
    let icon_file = data.join("icons/hicolor/scalable/apps/laurabrod.svg");
    if let Some(dir) = icon_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::copy(icon, &icon_file).map_err(|e| format!("{}: {e}", icon_file.display()))?;
    let entry = data.join("applications/laurabrod.desktop");
    write(&entry, &desktop_entry(binary, "laurabrod"), false)?;
    let apps = entry.parent().unwrap_or(&data).display().to_string();
    quiet("update-desktop-database", &[&apps]);
    Ok(vec![entry, icon_file])
}

/// Launcher on the desktop (marked trusted where GNOME supports it).
pub fn desktop_shortcut(binary: &Path, icon: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = dirs::desktop_dir().ok_or("нет рабочего стола")?;
    let entry = dir.join("laurabrod.desktop");
    write(&entry, &desktop_entry(binary, &icon.display().to_string()), true)?;
    quiet("gio", &["set", &entry.display().to_string(), "metadata::trusted", "true"]);
    Ok(vec![entry])
}

/// `~/.local/bin/laurabrod` → the installed binary (that directory is on the
/// PATH in most distributions).
pub fn add_to_path(_dir: &Path, binary: &Path) -> Result<PathChange, String> {
    let bin = dirs::home_dir().ok_or("нет домашнего каталога")?.join(".local/bin");
    std::fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let link = bin.join("laurabrod");
    if let Ok(meta) = std::fs::symlink_metadata(&link) {
        if !meta.file_type().is_symlink() {
            return Err(format!("{} уже существует и это не ссылка", link.display()));
        }
        let _ = std::fs::remove_file(&link);
    }
    std::os::unix::fs::symlink(binary, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    Ok(PathChange { entry: None, extras: vec![link] })
}

pub fn remove_from_path(_dir: &Path) -> Result<(), String> {
    Ok(())
}

pub fn register(_: &Registration) -> Result<bool, String> {
    Ok(false)
}

pub fn unregister() {}

/// Unix can delete a running executable, so the leftovers go right away.
pub fn delete_later(files: &[PathBuf], dir: &Path) {
    for f in files {
        let _ = std::fs::remove_file(f);
    }
    let _ = std::fs::remove_dir(dir);
}

/// Start the installed application as if from its menu entry.
pub fn launch(binary: &Path) -> Result<(), String> {
    Command::new(binary)
        .current_dir(binary.parent().unwrap_or(Path::new("/")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", binary.display()))
}

pub fn attach_console() {}

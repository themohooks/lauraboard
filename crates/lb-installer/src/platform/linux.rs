//! Linux (and other Unix desktops): freedesktop menu entry and icon,
//! desktop launcher, a `lauraboard` link on the PATH. Per user: all inside
//! the home directory; for all users: `/opt`, `/usr/share`, `/usr/local/bin`
//! (run as root).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{APP_COMMENT, APP_NAME, PathChange, Registration};

pub fn no_window(_: &mut Command) {}

fn data_dir(all_users: bool) -> Result<PathBuf, String> {
    if all_users {
        return Ok(PathBuf::from("/usr/share"));
    }
    dirs::data_local_dir().ok_or_else(|| "нет каталога ~/.local/share".to_string())
}

/// Parent of a new installation: `~/.local/share` or `/opt`.
pub fn programs_dir(all_users: bool) -> Option<PathBuf> {
    if all_users { Some(PathBuf::from("/opt")) } else { dirs::data_local_dir() }
}

/// `/var/lib/lauraboard` — the receipt of an all-users installation.
pub fn machine_data_dir() -> Option<PathBuf> {
    Some(PathBuf::from("/var/lib/lauraboard"))
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

/// Applications menu: `<data>/applications/lauraboard.desktop` plus the
/// icon in the hicolor theme.
pub fn menu_shortcut(binary: &Path, icon: &Path, all_users: bool) -> Result<Vec<PathBuf>, String> {
    let data = data_dir(all_users)?;
    let icon_file = data.join("icons/hicolor/scalable/apps/lauraboard.svg");
    if let Some(dir) = icon_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::copy(icon, &icon_file).map_err(|e| format!("{}: {e}", icon_file.display()))?;
    let entry = data.join("applications/lauraboard.desktop");
    write(&entry, &desktop_entry(binary, "lauraboard"), false)?;
    let apps = entry.parent().unwrap_or(&data).display().to_string();
    quiet("update-desktop-database", &[&apps]);
    Ok(vec![entry, icon_file])
}

/// Launcher on the desktop (marked trusted where GNOME supports it) — of
/// the user running the installer: there is no shared desktop.
pub fn desktop_shortcut(binary: &Path, icon: &Path, _all_users: bool) -> Result<Vec<PathBuf>, String> {
    // without xdg-user-dirs configured, fall back to ~/Desktop if it exists
    let dir =
        dirs::desktop_dir().or_else(|| dirs::home_dir().map(|h| h.join("Desktop")).filter(|d| d.is_dir())).ok_or("нет рабочего стола")?;
    let entry = dir.join("lauraboard.desktop");
    write(&entry, &desktop_entry(binary, &icon.display().to_string()), true)?;
    quiet("gio", &["set", &entry.display().to_string(), "metadata::trusted", "true"]);
    Ok(vec![entry])
}

/// `~/.local/bin/lauraboard` (or `/usr/local/bin/lauraboard`) → the
/// installed binary (that directory is on the PATH in most distributions).
pub fn add_to_path(_dir: &Path, binary: &Path, all_users: bool) -> Result<PathChange, String> {
    let bin = if all_users {
        PathBuf::from("/usr/local/bin")
    } else {
        dirs::home_dir().ok_or("нет домашнего каталога")?.join(".local/bin")
    };
    std::fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let link = bin.join("lauraboard");
    if let Ok(meta) = std::fs::symlink_metadata(&link) {
        if !meta.file_type().is_symlink() {
            return Err(format!("{} уже существует и это не ссылка", link.display()));
        }
        let _ = std::fs::remove_file(&link);
    }
    std::os::unix::fs::symlink(binary, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    Ok(PathChange { entry: None, extras: vec![link] })
}

pub fn remove_from_path(_dir: &Path, _all_users: bool) -> Result<(), String> {
    Ok(())
}

pub fn register(_: &Registration) -> Result<bool, String> {
    Ok(false)
}

pub fn unregister(_all_users: bool) {}

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

/// Running as root.
pub fn elevated() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .is_ok_and(|s| s.lines().any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some("0")))
}

/// A graphical session cannot be re-started as root reliably (pkexec drops
/// the display), so the user is asked to do it.
pub fn elevate(_args: &[std::ffi::OsString], _wait: bool) -> Result<Option<u32>, String> {
    Err("Для установки для всех пользователей запустите установщик от имени root: sudo ./lauraboard-setup".into())
}

/// Wait (up to `timeout`) until process `pid` has exited.
pub fn wait_exit(pid: u32, timeout: std::time::Duration) {
    let end = std::time::Instant::now() + timeout;
    while Path::new(&format!("/proc/{pid}")).exists() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

//! This computer's side of updates: how the running copy was installed,
//! where a download goes, and what "install" means for it.

use std::path::{Path, PathBuf};

use super::release::{Asset, Kind};

/// Folder the setup program installed into (its receipt,
/// `<config dir>/lauraboard/installation.json`).
fn installed_dir() -> Option<PathBuf> {
    let path = dirs::config_dir()?.join("lauraboard").join("installation.json");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(PathBuf::from(v["dir"].as_str()?))
}

fn same_dir(exe: &Path, dir: &Path) -> bool {
    match (exe.parent().and_then(|p| p.canonicalize().ok()), dir.canonicalize().ok()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// How the running copy was installed.
pub fn detect() -> Kind {
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(windows) {
        return if installed_dir().is_some_and(|d| same_dir(&exe, &d)) { Kind::Setup } else { Kind::Portable };
    }
    if cfg!(target_os = "linux") {
        if let Some(p) = std::env::var_os("APPIMAGE").filter(|p| !p.is_empty()) {
            return Kind::AppImage(p.into());
        }
        if installed_dir().is_some_and(|d| same_dir(&exe, &d)) {
            return Kind::Setup;
        }
        if exe.starts_with("/usr") || exe.starts_with("/opt") {
            if Path::new("/usr/bin/dpkg").exists() {
                return Kind::Deb;
            }
            if Path::new("/usr/bin/rpm").exists() {
                return Kind::Rpm;
            }
            return Kind::Page;
        }
        return Kind::Portable;
    }
    if cfg!(target_os = "macos") {
        return Kind::Dmg;
    }
    Kind::Page
}

/// Where the downloaded file is kept until it is installed.
pub fn destination(kind: &Kind, asset: &Asset) -> PathBuf {
    match kind {
        // next to the running AppImage (same disk: the swap is a rename)
        Kind::AppImage(p) => p.with_file_name(format!(".{}.new", asset.name)),
        Kind::Setup => std::env::temp_dir().join("lauraboard-update").join(&asset.name),
        _ => dirs::download_dir().unwrap_or_else(std::env::temp_dir).join(&asset.name),
    }
}

/// What the "install" button does, shown before it is pressed.
pub fn install_hint(kind: &Kind) -> &'static str {
    match kind {
        Kind::Setup => "Программа закроется и откроется установщик новой версии — настройки и проекты сохранятся.",
        Kind::AppImage(_) => "Файл AppImage будет заменён новым, программа перезапустится.",
        Kind::Portable => "Архив новой версии будет скачан в «Загрузки» — закройте программу и распакуйте его поверх её папки.",
        Kind::Dmg => "Откроется образ DMG новой версии. Закройте LauraBoard и перенесите приложение в «Программы»; проекты сохранятся.",
        Kind::Deb | Kind::Rpm => "Пакет новой версии будет скачан в «Загрузки» и открыт в установщике пакетов системы.",
        Kind::Page => "Откроется страница выпуска на GitHub.",
    }
}

/// Install a verified download. `Ok(true)`: the program must quit now.
pub fn install(kind: &Kind, file: &Path) -> Result<bool, String> {
    match kind {
        Kind::Setup => {
            executable(file)?;
            std::process::Command::new(file).spawn().map_err(|e| format!("не удалось запустить установщик: {e}"))?;
            Ok(true)
        }
        Kind::AppImage(target) => {
            executable(file)?;
            // a running AppImage may be replaced: the old inode stays open
            std::fs::rename(file, target).map_err(|e| format!("не удалось заменить {}: {e}", target.display()))?;
            std::process::Command::new(target).spawn().map_err(|e| format!("не удалось запустить новую версию: {e}"))?;
            Ok(true)
        }
        Kind::Deb | Kind::Rpm | Kind::Dmg => open(file).map(|_| false),
        Kind::Portable => reveal(file).map(|_| false),
        Kind::Page => Ok(false),
    }
}

/// Downloads come without the executable bit (Unix).
fn executable(file: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    let _ = file;
    Ok(())
}

/// Open a file or a web page with the system's default application.
pub fn open(target: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(target).spawn().map(|_| ()).map_err(|e| format!("не удалось открыть: {e}"))
}

/// Show a file selected in the file manager.
pub fn reveal(file: &Path) -> Result<(), String> {
    if cfg!(windows) {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(file);
        std::process::Command::new("explorer").arg(arg).spawn().map(|_| ()).map_err(|e| e.to_string())
    } else {
        open(file.parent().unwrap_or(file))
    }
}

//! The application packed into the installer: a zip archive embedded at
//! build time (see `build.rs`) with `manifest.json` at its root.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zip"));

pub const MANIFEST: &str = "manifest.json";

/// `manifest.json`, written by `cargo dist`.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub os: String,
    pub arch: String,
    /// Main executable, relative to the install directory.
    pub binary: String,
    /// Icon for menus and shortcuts, relative to the install directory.
    pub icon: String,
    /// Uninstaller, relative to the install directory.
    pub uninstaller: String,
    /// Unpacked size in bytes.
    pub size: u64,
}

/// `true` when this binary carries the application (else: uninstaller only).
pub fn present() -> bool {
    !PAYLOAD.is_empty()
}

fn archive() -> Result<zip::ZipArchive<Cursor<&'static [u8]>>, String> {
    zip::ZipArchive::new(Cursor::new(PAYLOAD)).map_err(|e| format!("пакет повреждён: {e}"))
}

pub fn manifest() -> Result<Manifest, String> {
    let mut zip = archive()?;
    let mut file = zip.by_name(MANIFEST).map_err(|e| format!("в пакете нет {MANIFEST}: {e}"))?;
    let mut json = String::new();
    file.read_to_string(&mut json).map_err(|e| e.to_string())?;
    let m: Manifest = serde_json::from_str(&json).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    if m.os != os || m.arch != arch {
        return Err(format!("установщик собран для {}/{}, а это {os}/{arch}", m.os, m.arch));
    }
    Ok(m)
}

/// Unpack everything except the manifest into `dir`. Reports unpacked bytes
/// to `progress`; returns the written paths relative to `dir`, `/`-separated.
pub fn extract(dir: &Path, progress: &mut dyn FnMut(u64)) -> Result<Vec<String>, String> {
    let mut zip = archive()?;
    let mut written = Vec::new();
    let mut done = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        if rel == Path::new(MANIFEST) || entry.is_dir() {
            continue;
        }
        let target = dir.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        replace_file(&target, &mut entry)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o777));
        }
        done += entry.size();
        progress(done);
        written.push(rel.to_string_lossy().replace('\\', "/"));
    }
    Ok(written)
}

/// Write a file even when the old one is in use: a running Windows
/// executable cannot be overwritten but can be renamed out of the way
/// (`*.old`, removed by the next install or the uninstaller).
fn replace_file(target: &Path, data: &mut dyn Read) -> Result<(), String> {
    if target.exists() && std::fs::remove_file(target).is_err() {
        let old = old_name(target);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(target, &old).map_err(|e| format!("{} занят ({e}). Закройте LauraBoard и повторите.", target.display()))?;
    }
    let mut out = std::fs::File::create(target).map_err(|e| format!("{}: {e}", target.display()))?;
    std::io::copy(data, &mut out).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(())
}

/// `<path>.old` — where a busy file was moved.
pub fn old_name(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".old");
    name.into()
}

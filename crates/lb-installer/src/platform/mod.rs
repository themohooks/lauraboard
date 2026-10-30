//! OS integration: shortcuts / menu entries, PATH, "Apps & features".
//! Every function returns what it created so the uninstaller can undo it.

#[cfg(not(windows))]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use linux::*;
#[cfg(windows)]
pub use windows::*;

use std::path::PathBuf;

/// Result of putting the app on the PATH.
#[derive(Debug, Default)]
pub struct PathChange {
    /// Directory added to the user's PATH variable (Windows).
    pub entry: Option<PathBuf>,
    /// Files created for it (a symlink in `~/.local/bin` on Linux).
    pub extras: Vec<PathBuf>,
}

/// Data for the "Apps & features" entry (Windows).
#[derive(Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Registration<'a> {
    pub version: &'a str,
    pub dir: &'a std::path::Path,
    pub binary: &'a std::path::Path,
    pub uninstaller: &'a std::path::Path,
    pub size: u64,
    /// HKLM instead of HKCU.
    pub all_users: bool,
}

pub const APP_NAME: &str = "LauraBoard";
pub const APP_COMMENT: &str = "Автоматизация вещания интернет-телеканала";

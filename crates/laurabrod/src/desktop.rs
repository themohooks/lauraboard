//! Start from a file manager or a shortcut (no terminal): open the GUI.
//!
//! Started without arguments outside a terminal (double click on the
//! executable, Start menu / desktop shortcut), `laurabrod` behaves like
//! `laurabrod gui` on the remembered project. In a terminal nothing changes.
//!
//! The remembered project lives in `<config dir>/laurabrod/desktop.json`
//! (`{"project": "<dir>"}`); the installer writes the same file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `true` when no terminal is attached to the process.
///
/// Windows: a console program started from Explorer gets a console of its
/// own, so it is the only process attached to it (none at all when started
/// without a console, e.g. by the installer). Elsewhere: neither stdin
/// nor stderr is a terminal.
pub fn outside_terminal() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::GetConsoleProcessList;
        let mut pids = [0u32; 2];
        // SAFETY: the buffer is valid for `pids.len()` entries.
        let n = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
        n <= 1
    }
    #[cfg(not(windows))]
    {
        use std::io::IsTerminal;
        !std::io::stdin().is_terminal() && !std::io::stderr().is_terminal()
    }
}

/// Close the console window Windows created for us (no-op elsewhere).
pub fn detach_console() {
    #[cfg(windows)]
    {
        // SAFETY: plain Win32 call; later writes to stdio are ignored by std.
        unsafe { windows_sys::Win32::System::Console::FreeConsole() };
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct DesktopState {
    project: Option<PathBuf>,
}

fn state_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("laurabrod").join("desktop.json"))
}

fn load_state() -> DesktopState {
    state_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Remember the project the GUI was opened with (best effort).
pub fn remember(project: &Path) {
    let Some(path) = state_path() else { return };
    let project = std::path::absolute(project).unwrap_or_else(|_| project.to_path_buf());
    let state = DesktopState { project: Some(project) };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(&state) {
        let _ = std::fs::write(path, json);
    }
}

/// Project for a desktop start: the current directory if it is a project,
/// else the remembered one, else `<Documents>/Laurabrod`.
pub fn project() -> PathBuf {
    if Path::new(lb_core::project::APP_FILE).exists() {
        return PathBuf::from(".");
    }
    load_state().project.unwrap_or_else(default_project)
}

/// `<Documents>/Laurabrod` (home directory when there is no Documents folder).
pub fn default_project() -> PathBuf {
    dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("Laurabrod")
}

/// Show an error where the user can see it: there is no console.
pub fn show_error(message: &str) {
    tracing::error!("{message}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Laurabrod")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

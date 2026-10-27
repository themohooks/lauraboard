//! Finding a Chromium-based browser (Edge, Chrome, Chromium): HTML
//! graphics are captured with it, and sites are signed in to in its window.

use std::path::PathBuf;

/// `LAURABOARD_BROWSER`, otherwise the usual install locations.
pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LAURABOARD_BROWSER") {
        return Some(p.into());
    }
    let mut paths = Vec::new();
    for env in ["PROGRAMFILES(X86)", "PROGRAMFILES", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(env) {
            for suffix in ["Microsoft/Edge/Application/msedge.exe", "Google/Chrome/Application/chrome.exe"] {
                paths.push(PathBuf::from(&base).join(suffix));
            }
        }
    }
    paths.extend(
        [
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/google-chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ]
        .map(PathBuf::from),
    );
    paths.into_iter().find(|p| p.is_file())
}

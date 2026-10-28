//! The private web runtime shipped with LauraBoard. Never searches installed
//! browsers, PATH or system WebView runtimes.

use std::path::PathBuf;

pub const MISSING: &str = "Не найден движок LauraBoard в runtime/browser. Переустановите полную версию приложения.";

pub fn path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "chrome.exe"
    } else if cfg!(target_os = "macos") {
        "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
    } else {
        "chrome"
    };
    let parent = exe.parent()?;
    let parent = if parent.file_name().is_some_and(|p| p == "deps") { parent.parent()? } else { parent };
    let path = parent.join("runtime/browser").join(name);
    path.is_file().then_some(path)
}

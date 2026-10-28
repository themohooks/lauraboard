//! Locate the small native WebView host and offline SWF player.
use std::path::PathBuf;

pub const MISSING: &str = "Не найден lauraboard-webview. Переустановите приложение вместе с компонентом WebView.";

pub fn path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    let parent = if parent.file_name().is_some_and(|p| p == "deps") { parent.parent()? } else { parent };
    let path = parent.join(if cfg!(windows) { "lauraboard-webview.exe" } else { "lauraboard-webview" });
    path.is_file().then_some(path)
}

pub fn ruffle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    let resources = parent.parent()?.join("Resources/ruffle");
    if cfg!(target_os = "macos") && resources.join("ruffle.js").is_file() {
        return Some(resources);
    }
    let parent = if parent.file_name().is_some_and(|p| p == "deps") { parent.parent()? } else { parent };
    let path = parent.join("runtime/ruffle");
    path.join("ruffle.js").is_file().then_some(path)
}

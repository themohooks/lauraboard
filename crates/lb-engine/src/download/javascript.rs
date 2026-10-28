//! yt-dlp challenge provider backed by LauraBoard's isolated system WebView.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::tool::TOOLS_DIR;

const PROVIDER: &str = include_str!("webview_jsc.py");
static PREPARE: Mutex<()> = Mutex::new(());

fn plugins(root: &Path) -> PathBuf {
    root.join(TOOLS_DIR).join("webview-plugins")
}

pub fn prepare(root: &Path) -> Result<(), String> {
    if lb_core::browser::path().is_none() {
        return Ok(());
    }
    let _guard = PREPARE.lock().unwrap_or_else(|e| e.into_inner());
    let path = plugins(root).join("lauraboard/yt_dlp_plugins/extractor/lauraboard_webview.py");
    if std::fs::read_to_string(&path).ok().as_deref() == Some(PROVIDER) {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, PROVIDER).map_err(|e| format!("YouTube WebView provider: {e}"))
}

pub fn args(root: &Path) -> Vec<OsString> {
    if lb_core::browser::path().is_none() {
        return Vec::new();
    }
    vec!["--plugin-dirs".into(), plugins(root).into(), "--no-js-runtimes".into()]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_plugin_path_does_not_need_shell_quoting() {
        let path = plugins(Path::new("project with spaces"));
        assert!(path.ends_with("tools/webview-plugins"));
        assert!(PROVIDER.contains("class LauraBoardWebViewJCP"));
        assert!(!PROVIDER.contains("shell=True"));
    }
}

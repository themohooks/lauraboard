//! Signing in to sites for yt-dlp (YouTube asks for it on age-restricted
//! videos: "Sign in to confirm your age"). yt-dlp cannot log in with a
//! password there; it uses cookies from LauraBoard's private login window
//! or an imported `cookies.txt`. The choice is kept in
//! `<project>/tools` so it applies to every download of the project.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::tool::TOOLS_DIR;

const SETTINGS: &str = "yt-dlp-login.json";
const COOKIES: &str = "yt-dlp-cookies.txt";
/// Largest `cookies.txt` accepted (they are a few KB to a few hundred KB).
pub const MAX_COOKIES: usize = 4 * 1024 * 1024;

/// What the API reports and accepts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Login {
    /// Legacy API field. System browser profiles are no longer used.
    #[serde(default)]
    pub browser: Option<String>,
    /// A `cookies.txt` is stored (it wins over the browser).
    #[serde(default)]
    pub cookies: bool,
}

fn settings_path(root: &Path) -> PathBuf {
    root.join(TOOLS_DIR).join(SETTINGS)
}

fn cookies_path(root: &Path) -> PathBuf {
    root.join(TOOLS_DIR).join(COOKIES)
}

/// The current choice.
pub fn load(root: &Path) -> Login {
    let mut l: Login = std::fs::read(settings_path(root)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    l.browser = None;
    l.cookies = cookies_path(root).is_file();
    l
}

/// Use `browser` (`None`: none); `cookies`: `Some("")` removes the stored
/// file, `Some(text)` replaces it, `None` keeps it.
pub fn save(root: &Path, browser: Option<&str>, cookies: Option<&str>) -> Result<Login, String> {
    let browser = browser.map(str::trim).filter(|b| !b.is_empty());
    if browser.is_some() {
        return Err("system browser login is no longer supported; use LauraBoard login or cookies.txt".into());
    }
    let dir = root.join(TOOLS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    match cookies {
        Some("") => {
            let _ = std::fs::remove_file(cookies_path(root));
        }
        Some(text) => {
            check_cookies(text)?;
            std::fs::write(cookies_path(root), text).map_err(|e| e.to_string())?;
        }
        None => {}
    }
    let json = serde_json::to_vec_pretty(&Login { browser: browser.map(str::to_string), cookies: false }).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(root), json).map_err(|e| e.to_string())?;
    Ok(load(root))
}

/// A Netscape cookie file: the header or tab-separated lines of 7 fields.
fn check_cookies(text: &str) -> Result<(), String> {
    if text.len() > MAX_COOKIES {
        return Err("cookies file is too large".into());
    }
    let header = text.trim_start().starts_with("# Netscape HTTP Cookie File") || text.trim_start().starts_with("# HTTP Cookie File");
    let rows = text.lines().filter(|l| !l.trim().is_empty() && (!l.starts_with('#') || l.starts_with("#HttpOnly_")));
    if header || rows.clone().count() > 0 && rows.clone().all(|l| l.split('\t').count() == 7) {
        Ok(())
    } else {
        Err("not a cookies.txt file (Netscape format)".into())
    }
}

/// Arguments for yt-dlp.
pub fn args(root: &Path) -> Vec<OsString> {
    let l = load(root);
    if l.cookies {
        return vec!["--cookies".into(), cookies_path(root).into()];
    }
    Vec::new()
}

/// The site wants a signed-in user (age check, members-only, "not a bot").
pub fn needed(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["sign in to confirm", "confirm your age", "age-restricted", "--cookies", "login required", "use --cookies", "members-only"]
        .iter()
        .any(|k| e.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_browser_and_cookies() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(load(root), Login::default());
        assert!(args(root).is_empty());
        assert!(save(root, Some("firefox"), None).is_err());
        // Previously saved browser settings must no longer read system profiles.
        std::fs::create_dir_all(root.join(TOOLS_DIR)).unwrap();
        std::fs::write(settings_path(root), r#"{"browser":"firefox"}"#).unwrap();
        assert!(args(root).is_empty());
        let file = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tx\n";
        let l = save(root, None, Some(file)).unwrap();
        assert!(l.cookies);
        assert_eq!(args(root)[0], OsString::from("--cookies"));
        save(root, None, Some("")).unwrap();
        assert!(args(root).is_empty());
        assert!(save(root, Some("netscape"), None).is_err());
        assert!(save(root, None, Some("hello")).is_err());
    }

    #[test]
    fn recognises_sign_in_errors() {
        assert!(needed("[youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users."));
        assert!(needed("Use --cookies-from-browser or --cookies for the authentication."));
        assert!(!needed("[youtube] abc: Video unavailable"));
    }
}

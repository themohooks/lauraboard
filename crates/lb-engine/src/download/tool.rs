//! Finding, checking and installing yt-dlp.
//!
//! Lookup order for the default name: the updated copy in `<project>/tools`,
//! a copy shipped next to the program, then `PATH`. An explicit path in
//! `lauraboard.toml` (`[tools] ytdlp = "…"`) is used as it is.
//!
//! Installing downloads the official single-file build for this platform
//! from the yt-dlp GitHub releases and checks it against the release's
//! `SHA2-256SUMS` before it is put in place.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::jobs::JobCtl;

/// Sub-folder of the project for helper programs installed by LauraBoard.
pub const TOOLS_DIR: &str = "tools";
const DEFAULT_NAME: &str = "yt-dlp";
const RELEASE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const MAX_BYTES: u64 = 200 * 1024 * 1024;

/// What the API reports about the downloader.
#[derive(Debug, Clone, Serialize)]
pub struct ToolStatus {
    pub available: bool,
    /// `yt-dlp --version` (a date like `2026.09.30`).
    pub version: String,
    pub path: String,
    /// An official build exists for this platform (install / update button).
    pub can_install: bool,
    /// The copy in `<project>/tools` is ours: updating replaces it.
    pub installed_by_us: bool,
}

/// File name of the installed copy.
fn exe_name() -> String {
    format!("{DEFAULT_NAME}{}", std::env::consts::EXE_SUFFIX)
}

/// The copy installed by [`install`].
pub fn installed_path(project_root: &Path) -> PathBuf {
    project_root.join(TOOLS_DIR).join(exe_name())
}

/// Candidates in lookup order (explicit paths are used as they are).
fn candidates(configured: &str, project_root: &Path) -> Vec<PathBuf> {
    let name = configured.trim();
    let is_default = name.is_empty() || name == DEFAULT_NAME || name == exe_name();
    if !is_default {
        return vec![PathBuf::from(name)];
    }
    let mut out = vec![installed_path(project_root)];
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        out.push(dir.join(exe_name()));
    }
    out.push(PathBuf::from(DEFAULT_NAME));
    out
}

/// A background command: killed when dropped, no console window on Windows.
pub fn command(program: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    c.kill_on_drop(true).stdin(std::process::Stdio::null());
    if let Some(helper) = lb_core::browser::path() {
        c.env("LAURABOARD_WEBVIEW", helper);
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

async fn version_of(program: &Path) -> Option<String> {
    let mut cmd = command(program);
    cmd.arg("--version").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await.ok()?.ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !v.is_empty()).then_some(v)
}

/// The first working yt-dlp and its version.
pub async fn locate(configured: &str, project_root: &Path) -> Option<(PathBuf, String)> {
    for p in candidates(configured, project_root) {
        // a bare name is searched in PATH by the OS; a path must exist
        if p.components().count() > 1 && !p.is_file() {
            continue;
        }
        if let Some(v) = version_of(&p).await {
            return Some((p, v));
        }
    }
    None
}

pub async fn status(configured: &str, project_root: &Path) -> ToolStatus {
    let found = locate(configured, project_root).await;
    let ours = installed_path(project_root);
    ToolStatus {
        available: found.is_some(),
        version: found.as_ref().map(|f| f.1.clone()).unwrap_or_default(),
        installed_by_us: found.as_ref().is_some_and(|f| f.0 == ours),
        path: found.map(|f| f.0.display().to_string()).unwrap_or_default(),
        can_install: release_asset(std::env::consts::OS, std::env::consts::ARCH).is_some(),
    }
}

/// Name of the official single-file build for an OS / architecture.
pub fn release_asset(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("windows", "x86_64") => "yt-dlp.exe",
        ("windows", "x86") => "yt-dlp_x86.exe",
        ("windows", "aarch64") => "yt-dlp_arm64.exe",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("macos", _) => "yt-dlp_macos",
        _ => return None,
    })
}

/// Checksum of `asset` from a `SHA2-256SUMS` file (`<hex>  <name>` lines).
pub fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == asset && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

fn agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(600))).build();
    ureq::Agent::new_with_config(cfg)
}

/// Job body: download the official build into `<project>/tools`, verify
/// it and check that it runs. An older copy there is replaced.
pub async fn install(project_root: PathBuf, ctl: JobCtl) -> Result<Value, String> {
    let asset = release_asset(std::env::consts::OS, std::env::consts::ARCH).ok_or("no official yt-dlp build for this platform")?;
    ctl.progress(0.02, "SHA2-256SUMS");
    let sums_url = format!("{RELEASE_URL}/SHA2-256SUMS");
    let sums = tokio::task::spawn_blocking(move || {
        agent()
            .get(&sums_url)
            .header("User-Agent", "LauraBoard")
            .call()
            .map_err(|e| format!("{sums_url}: {e}"))?
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let expected = checksum_for(&sums, asset).ok_or_else(|| format!("{asset}: no checksum in SHA2-256SUMS"))?;

    let dest = installed_path(&project_root);
    let dir = dest.parent().map(Path::to_path_buf).ok_or("bad tools folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!("{}.download", exe_name()));
    let url = format!("{RELEASE_URL}/{asset}");
    let (t, c) = (tmp.clone(), ctl.clone());
    let digest = tokio::task::spawn_blocking(move || download_to(&url, &t, &c)).await.map_err(|e| e.to_string())?;
    let digest = match digest {
        Ok(d) => d,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    if digest != expected {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{asset}: checksum mismatch (download damaged or replaced)"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    ctl.progress(0.97, "проверка");
    let version = version_of(&tmp).await.ok_or("the downloaded yt-dlp does not start")?;
    // replace atomically where possible (Windows: the old file must go first)
    let _ = std::fs::remove_file(&dest);
    std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    tracing::info!(version, path = %dest.display(), "yt-dlp installed");
    super::javascript::prepare(&project_root)?;
    Ok(json!({ "version": version, "path": dest.display().to_string() }))
}

/// Stream `url` into `to`, reporting progress; returns the SHA-256 (hex).
fn download_to(url: &str, to: &Path, ctl: &JobCtl) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut res = agent().get(url).header("User-Agent", "LauraBoard").call().map_err(|e| format!("{url}: {e}"))?;
    let total: u64 = res.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    if total > MAX_BYTES {
        return Err(format!("{url}: file too large"));
    }
    let mut body = res.body_mut().with_config().limit(MAX_BYTES).reader();
    let mut file = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let started = std::time::Instant::now();
    loop {
        ctl.check()?;
        let n = body.read(&mut buf).map_err(|e| format!("{url}: {e}"))?;
        lb_core::traffic::received(lb_core::traffic::DOWNLOADS, n as u64);
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", to.display()))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if total > 0 {
            ctl.progress(0.05 + 0.9 * done as f32 / total as f32, format!("{} МБ", done / (1024 * 1024)));
        }
        ctl.speed(Some(done as f64 / started.elapsed().as_secs_f64().max(0.001)));
    }
    ctl.speed(None);
    file.flush().map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_per_platform() {
        assert_eq!(release_asset("windows", "x86_64"), Some("yt-dlp.exe"));
        assert_eq!(release_asset("linux", "aarch64"), Some("yt-dlp_linux_aarch64"));
        assert_eq!(release_asset("linux", "riscv64"), None);
    }

    #[test]
    fn checksum_lines() {
        let h = "a".repeat(64);
        let sums = format!("{}  yt-dlp_linux\n{h}  yt-dlp.exe\n", "b".repeat(64));
        assert_eq!(checksum_for(&sums, "yt-dlp.exe"), Some(h));
        assert_eq!(checksum_for(&sums, "yt-dlp"), None);
        assert_eq!(checksum_for("xyz  yt-dlp.exe", "yt-dlp.exe"), None);
    }

    #[test]
    fn explicit_path_is_the_only_candidate() {
        let root = Path::new("/p");
        assert_eq!(candidates("/opt/yt/yt-dlp", root), vec![PathBuf::from("/opt/yt/yt-dlp")]);
        let c = candidates("yt-dlp", root);
        assert_eq!(c.first(), Some(&installed_path(root)));
        assert_eq!(c.last(), Some(&PathBuf::from("yt-dlp")));
        assert!(c.contains(&installed_path(root)));
    }
}

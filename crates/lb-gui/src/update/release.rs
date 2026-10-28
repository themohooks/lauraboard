//! GitHub releases: the latest one, versions, which file fits this
//! computer and its checksum (pure, unit-tested).

use serde_json::Value;

/// Where releases are published.
pub const REPO: &str = "themohooks/lauraboard";

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// "1.4.0" (tag without the "v").
    pub version: String,
    /// Release page on GitHub.
    pub page: String,
    /// Release notes (markdown), shortened for the settings card.
    pub notes: String,
    pub assets: Vec<Asset>,
}

/// How this copy was installed: decides which file an update needs.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// The setup program's receipt lists this folder.
    Setup,
    /// Unpacked archive — the new archive goes to Downloads.
    Portable,
    /// Linux AppImage at this path: replaced in place.
    AppImage(std::path::PathBuf),
    /// Linux package managed by dpkg / rpm.
    Deb,
    Rpm,
    /// Native macOS disk image.
    Dmg,
    /// No matching downloadable package: open the release page.
    Page,
}

/// `GET /repos/{REPO}/releases/latest` (drafts and prereleases are never
/// "latest"; a prerelease tag is skipped anyway).
pub fn parse(v: &Value) -> Option<Release> {
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let version = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
    parts(&version)?;
    let assets = v["assets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a["name"].as_str()?.to_string(),
                        url: a["browser_download_url"].as_str()?.to_string(),
                        size: a["size"].as_u64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        version,
        page: v["html_url"].as_str().map(str::to_string).unwrap_or_else(|| format!("https://github.com/{REPO}/releases/latest")),
        notes: short_notes(v["body"].as_str().unwrap_or("")),
        assets,
    })
}

/// "1.2.3" → (1, 2, 3); prereleases ("1.2.3-rc.1") and other forms → None.
fn parts(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.');
    let p = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    it.next().is_none().then_some(p)
}

/// `latest` is a newer release than `current` (a prerelease build counts
/// as its base version, so 1.2.0-rc.1 → 1.2.0 is offered).
pub fn newer(latest: &str, current: &str) -> bool {
    let base = current.split(['-', '+']).next().unwrap_or(current);
    match (parts(latest), parts(base)) {
        (Some(l), Some(c)) => l > c || (l == c && base != current),
        _ => false,
    }
}

/// The change list the release script writes after the download table
/// (older releases: the text before that table).
fn short_notes(body: &str) -> String {
    let body = match body.split_once("## Изменения") {
        Some((_, changes)) => changes,
        None => body.split("## Скачать").next().unwrap_or(body),
    };
    let text: String = body
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim_start().starts_with('<') && !l.starts_with("[Все изменения"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    if text.chars().count() > 1200 { format!("{}…", text.chars().take(1200).collect::<String>().trim_end()) } else { text.to_string() }
}

/// Rust target arch → the name used in release files.
pub fn arch_name(arch: &str) -> &str {
    match arch {
        "x86" => "i686",
        a => a,
    }
}

/// The file an update of `kind` needs on `os` / `arch`
/// (`std::env::consts` names), see `.github/scripts/release.py`. Linux
/// releases have shipped either a setup program and a desktop archive or
/// AppImage / deb / rpm packages; `None` when this release lacks the kind.
pub fn pick<'a>(r: &'a Release, kind: &Kind, os: &str, arch: &str) -> Option<&'a Asset> {
    let v = &r.version;
    let target = |os: &str| format!("lauraboard-{v}-{}-{os}", arch_name(arch));
    let name = match (os, kind) {
        ("windows", Kind::Setup) => format!("lauraboard-{v}-windows-{arch}-setup.exe"),
        ("windows", _) => format!("{}-desktop.zip", target("pc-windows-msvc")),
        ("linux", Kind::Setup) => format!("lauraboard-{v}-linux-{arch}-setup"),
        ("linux", Kind::Portable) => format!("{}-desktop.tar.gz", target("unknown-linux-gnu")),
        ("linux", Kind::AppImage(_)) => format!("{}.AppImage", target("unknown-linux-gnu")),
        ("linux", Kind::Deb) => format!("{}.deb", target("unknown-linux-gnu")),
        ("macos", Kind::Dmg) => format!("{}.dmg", target("apple-darwin")),
        ("macos", Kind::Portable) => format!("{}-desktop.tar.gz", target("apple-darwin")),
        ("linux", Kind::Rpm) => format!("{}.rpm", target("unknown-linux-gnu")),
        _ => return None,
    };
    r.assets.iter().find(|a| a.name == name)
}

/// The SHA256SUMS file of the release.
pub fn sums(r: &Release) -> Option<&Asset> {
    r.assets.iter().find(|a| a.name == "SHA256SUMS")
}

/// Hex digest of `name` in a `sha256sum` listing ("<hex>  <name>" or
/// "<hex> *<name>").
pub fn checksum(listing: &str, name: &str) -> Option<String> {
    listing.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release() -> Release {
        let names = [
            "lauraboard-1.4.0-windows-x86_64-setup.exe",
            "lauraboard-1.4.0-x86_64-pc-windows-msvc-desktop.zip",
            "lauraboard-1.4.0-i686-pc-windows-msvc-desktop.zip",
            "lauraboard-1.4.0-aarch64-unknown-linux-gnu.AppImage",
            "lauraboard-1.4.0-x86_64-unknown-linux-gnu.deb",
            "lauraboard-1.4.0-linux-x86_64-setup",
            "lauraboard-1.4.0-x86_64-unknown-linux-gnu-desktop.tar.gz",
            "SHA256SUMS",
        ];
        let assets: Vec<Value> =
            names.iter().map(|n| json!({"name": n, "browser_download_url": format!("https://x/{n}"), "size": 10})).collect();
        parse(&json!({"tag_name": "v1.4.0", "html_url": "https://page", "body": "Новое\n\n## Скачать для вашей ОС\n<table>", "assets": assets})).unwrap()
    }

    #[test]
    fn parses_latest_and_skips_prereleases() {
        let r = release();
        assert_eq!(r.version, "1.4.0");
        assert_eq!(r.notes, "Новое");
        let notes = "## Скачать для вашей ОС
<table>
---

## Изменения

- Fix ([`abc`](u)) — Ann

[Все изменения v1…v2](u)
";
        assert_eq!(short_notes(notes), "- Fix ([`abc`](u)) — Ann");
        assert_eq!(r.assets.len(), 8);
        assert!(parse(&json!({"tag_name": "v2.0.0-rc.1"})).is_none());
        assert!(parse(&json!({"tag_name": "v2.0.0", "prerelease": true})).is_none());
    }

    #[test]
    fn compares_versions() {
        assert!(newer("1.4.0", "1.3.9"));
        assert!(newer("1.10.0", "1.9.0"));
        assert!(!newer("1.4.0", "1.4.0"));
        assert!(!newer("1.3.0", "1.4.0"));
        assert!(newer("1.4.0", "1.4.0-rc.2"));
        assert!(!newer("junk", "1.0.0"));
    }

    #[test]
    fn picks_the_file_for_this_computer() {
        let r = release();
        let name = |k: Kind, os, arch| pick(&r, &k, os, arch).map(|a| a.name.as_str());
        assert_eq!(name(Kind::Setup, "windows", "x86_64"), Some("lauraboard-1.4.0-windows-x86_64-setup.exe"));
        assert_eq!(name(Kind::Portable, "windows", "x86"), Some("lauraboard-1.4.0-i686-pc-windows-msvc-desktop.zip"));
        assert_eq!(name(Kind::AppImage("/a".into()), "linux", "aarch64"), Some("lauraboard-1.4.0-aarch64-unknown-linux-gnu.AppImage"));
        assert_eq!(name(Kind::Deb, "linux", "x86_64"), Some("lauraboard-1.4.0-x86_64-unknown-linux-gnu.deb"));
        assert_eq!(name(Kind::Setup, "linux", "x86_64"), Some("lauraboard-1.4.0-linux-x86_64-setup"));
        assert_eq!(name(Kind::Portable, "linux", "x86_64"), Some("lauraboard-1.4.0-x86_64-unknown-linux-gnu-desktop.tar.gz"));
        assert_eq!(name(Kind::Rpm, "linux", "x86_64"), None);
        assert_eq!(name(Kind::Setup, "windows", "aarch64"), None);
        assert_eq!(name(Kind::Page, "macos", "aarch64"), None);
    }

    #[test]
    fn reads_checksums() {
        let h = "AB".repeat(32);
        let listing = format!("{h}  lauraboard-1.4.0-x86_64-unknown-linux-gnu.deb\n{} *other.zip\n", "cd".repeat(32));
        assert_eq!(checksum(&listing, "lauraboard-1.4.0-x86_64-unknown-linux-gnu.deb"), Some(h.to_lowercase()));
        assert_eq!(checksum(&listing, "other.zip"), Some("cd".repeat(32)));
        assert_eq!(checksum(&listing, "missing"), None);
    }
}

//! Channel export to a ZIP archive and import from one.
//!
//! ```text
//! manifest.json            what is inside (format, channel, parts)
//! channel/channel.toml     the channel folder as on disk
//! channel/library.json
//! channel/playlists/*.json
//! channel/schedule.toml, rules.toml, graphics.toml
//! channel/media/…          media files (own copies and, with `media`,
//!                          referenced files from elsewhere)
//! channel/asrun/…          as-run logs (with `history`)
//! ```
//!
//! * [`export`] — archive a channel (background job)
//! * [`import`] — create a channel from an archive (background job)

pub mod export;
pub mod import;

use serde::{Deserialize, Serialize};

/// Archive format marker.
pub const FORMAT: &str = "laurabrod-channel";
pub const VERSION: u32 = 1;
pub const MANIFEST: &str = "manifest.json";
/// Folder of the channel inside the archive.
pub const CHANNEL_DIR: &str = "channel/";

/// What goes into an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parts {
    /// Video / audio / encoder / planning settings (channel.toml).
    #[serde(default = "yes")]
    pub settings: bool,
    /// Output targets with their stream keys.
    #[serde(default)]
    pub outputs: bool,
    #[serde(default = "yes")]
    pub library: bool,
    /// Media files themselves (own copies and referenced files).
    #[serde(default = "yes")]
    pub media: bool,
    #[serde(default = "yes")]
    pub playlists: bool,
    #[serde(default = "yes")]
    pub schedules: bool,
    #[serde(default = "yes")]
    pub rules: bool,
    #[serde(default = "yes")]
    pub graphics: bool,
    /// As-run logs (statistics history).
    #[serde(default)]
    pub history: bool,
}

fn yes() -> bool {
    true
}

impl Default for Parts {
    fn default() -> Self {
        Parts {
            settings: true,
            outputs: false,
            library: true,
            media: true,
            playlists: true,
            schedules: true,
            rules: true,
            graphics: true,
            history: false,
        }
    }
}

/// `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    /// Channel id at export time (paths inside are relative to it).
    pub channel: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub exported_at: String,
    #[serde(default)]
    pub app_version: String,
    pub parts: Parts,
    /// Number of media files and their total size.
    #[serde(default)]
    pub media_files: usize,
    #[serde(default)]
    pub media_bytes: u64,
}

/// Read the manifest of an archive (to show what it contains).
pub fn inspect(path: &std::path::Path) -> Result<Manifest, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: not a ZIP archive ({e})", path.display()))?;
    read_manifest(&mut zip)
}

fn read_manifest<R: std::io::Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> Result<Manifest, String> {
    let entry = zip.by_name(MANIFEST).map_err(|_| "not a Laurabrod channel archive (no manifest.json)".to_string())?;
    let m: Manifest = serde_json::from_reader(entry).map_err(|e| format!("manifest.json: {e}"))?;
    if m.format != FORMAT {
        return Err("not a Laurabrod channel archive".into());
    }
    if m.version > VERSION {
        return Err(format!("archive format {} is newer than this program supports ({VERSION})", m.version));
    }
    Ok(m)
}

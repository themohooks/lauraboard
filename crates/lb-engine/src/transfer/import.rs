//! Import: unpack an archive into a new channel folder (next to the
//! others, then renamed into place so a failed import leaves nothing
//! behind), point its paths to the new id and start the channel's actor.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use lb_core::project::{CHANNELS_DIR, ChannelData};

use super::{CHANNEL_DIR, read_manifest};
use crate::engine::Engine;
use crate::jobs::JobCtl;

/// Unpack `channel/…` entries into `dest`, refusing paths that would
/// escape it. Returns the manifest.
fn unpack(zip_path: &Path, dest: &Path, ctl: &JobCtl) -> Result<super::Manifest, String> {
    let f = File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: not a ZIP archive ({e})", zip_path.display()))?;
    let manifest = read_manifest(&mut zip)?;
    let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size())).sum::<u64>().max(1);
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    for i in 0..zip.len() {
        ctl.check()?;
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Some(inner) = rel.strip_prefix(CHANNEL_DIR) else { continue };
        if inner.is_empty() || entry.is_dir() {
            continue;
        }
        let target: PathBuf = dest.join(inner);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        ctl.progress(done as f32 / total as f32, inner.to_string());
        let mut out = File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        loop {
            ctl.check()?;
            let k = entry.read(&mut buf).map_err(|e| format!("{inner}: {e}"))?;
            if k == 0 {
                break;
            }
            out.write_all(&buf[..k]).map_err(|e| format!("{}: {e}", target.display()))?;
            done += k as u64;
        }
    }
    Ok(manifest)
}

impl Engine {
    /// Job body: create channel `id` (or the archive's id) from an archive.
    pub async fn import_channel(
        self: Arc<Self>,
        zip_path: PathBuf,
        id: Option<String>,
        name: Option<String>,
        ctl: JobCtl,
    ) -> Result<Value, String> {
        let manifest = super::inspect(&zip_path)?;
        let id = id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| manifest.channel.clone());
        let final_dir = self.check_new_id(&id).map_err(|e| e.to_string())?;
        let staging = self.project_root.join(CHANNELS_DIR).join(format!(".import-{id}-{}", uuid::Uuid::new_v4().simple()));
        let (zp, st, c2) = (zip_path.clone(), staging.clone(), ctl.clone());
        let unpacked = tokio::task::spawn_blocking(move || unpack(&zp, &st, &c2)).await.map_err(|e| e.to_string())?;
        let finish = || -> Result<ChannelData, String> {
            let manifest = unpacked?;
            ctl.progress(0.99, "loading the channel");
            let mut data = ChannelData::load(&staging).map_err(|e| e.to_string())?;
            data.rebase_paths(&manifest.channel, &id);
            data.config.id = id.clone();
            if let Some(n) = name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                data.config.name = n.to_string();
            }
            data.config.enabled = true;
            data.config.autostart = false;
            data.save().map_err(|e| e.to_string())?;
            // the id may have been taken while unpacking
            self.check_new_id(&id).map_err(|e| e.to_string())?;
            std::fs::rename(&staging, &final_dir).map_err(|e| format!("{}: {e}", final_dir.display()))?;
            ChannelData::load(&final_dir).map_err(|e| e.to_string())
        };
        let data = match finish() {
            Ok(d) => d,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
        };
        let items = data.library.items.len();
        self.edit_channel_list(|l| l.push(id.clone())).map_err(|e| e.to_string())?;
        self.register(data);
        tracing::info!(channel = %id, archive = %zip_path.display(), "channel imported");
        Ok(json!({ "channel": id, "media": items }))
    }

    /// Job body: write channel `id` to an archive.
    pub async fn export_channel(self: Arc<Self>, id: String, out: PathBuf, parts: super::Parts, ctl: JobCtl) -> Result<Value, String> {
        let h = self.channel(&id).map_err(|e| e.to_string())?;
        let data = h.read(|d| d.clone());
        let root = self.project_root.clone();
        tokio::task::spawn_blocking(move || super::export::run(data, &root, &out, parts, &ctl)).await.map_err(|e| e.to_string())?
    }
}

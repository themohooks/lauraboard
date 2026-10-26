//! The channel's own media storage: imported files are copied into
//! `channels/<id>/media/<uuid>.<ext>`, so the library no longer depends on
//! the originals (moved, renamed, on a removable disk). The library keeps
//! the original file name and an editable title.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use lb_core::media::is_url;
use lb_core::project::paths;
use lb_core::project::rebase::channel_prefix;

use super::{IMAGE_EXT, ImportOptions, expand_inputs, ext, new_item, probe_item};
use crate::channel::ChannelHandle;
use crate::jobs::{JobCtl, copy_with_progress};

/// Sub-folder of a channel holding its copies.
pub const MEDIA_DIR: &str = "media";

/// The stored path points into the channel's own storage (the file is
/// ours and goes away with the library item).
pub fn is_own_file(channel: &str, stored: &str) -> bool {
    stored.replace('\\', "/").starts_with(&format!("{}{MEDIA_DIR}/", channel_prefix(channel)))
}

/// Delete a copied file when its library item is removed.
pub fn remove_own_file(root: &Path, channel: &str, stored: &str) {
    if is_own_file(channel, stored)
        && let Err(e) = std::fs::remove_file(paths::resolve(root, stored))
    {
        tracing::warn!(channel, file = stored, "could not delete media copy: {e}");
    }
}

fn storage_name(src: &Path) -> String {
    let e = ext(src);
    let id = uuid::Uuid::new_v4().simple().to_string();
    if e.is_empty() { id } else { format!("{id}.{e}") }
}

/// Job body: copy files (URLs are added as they are) into the channel's
/// storage, probe each copy and add it to the library one by one, so a
/// cancelled import keeps what was finished.
pub async fn import_copies(
    h: ChannelHandle,
    ffprobe: String,
    inputs: Vec<String>,
    opt: ImportOptions,
    ctl: JobCtl,
) -> Result<Value, String> {
    let root = h.shared.root.clone();
    let channel = h.id().to_string();
    let dir = h.read(|d| d.dir.join(MEDIA_DIR));
    let files = expand_inputs(&inputs, opt.recursive);
    if files.is_empty() {
        return Err("no media files found".into());
    }
    let sizes: Vec<u64> = files.iter().map(|f| if is_url(f) { 0 } else { std::fs::metadata(f).map(|m| m.len()).unwrap_or(0) }).collect();
    let total = sizes.iter().sum::<u64>().max(1);
    let count = files.len();
    let mut done: u64 = 0;
    let (mut added, mut errors) = (Vec::new(), Vec::new());
    for (i, f) in files.iter().enumerate() {
        ctl.check()?;
        let src = PathBuf::from(f);
        let name = if is_url(f) { f.clone() } else { src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
        ctl.progress(done as f32 / total as f32, format!("{}/{count}: {name}", i + 1));
        let (stored, stem) = if is_url(f) {
            (f.clone(), f.rsplit('/').next().unwrap_or("stream").to_string())
        } else {
            let dest = dir.join(storage_name(&src));
            let (from, to, c2) = (src.clone(), dest.clone(), ctl.clone());
            let base = done;
            let copied = tokio::task::spawn_blocking(move || {
                let mut n = 0u64;
                copy_with_progress(&from, &to, &c2, |b| {
                    n += b;
                    c2.progress((base + n) as f32 / total as f32, format!("{}/{count}: {name}", i + 1));
                })
            })
            .await
            .map_err(|e| e.to_string())?;
            if let Err(e) = copied {
                ctl.check()?;
                errors.push(format!("{f}: {e}"));
                continue;
            }
            let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "item".into());
            (paths::to_stored(&root, &dest), stem)
        };
        done += sizes[i];
        let is_image = IMAGE_EXT.contains(&ext(&src).as_str());
        let mut item = h.read(|d| new_item(&d.library, &stored, &stem, is_url(f), is_image, &opt));
        if !is_url(f) {
            item.original_name = src.file_name().map(|n| n.to_string_lossy().into_owned());
        }
        if let Err(e) = probe_item(&mut item, &root, &ffprobe).await {
            remove_own_file(&root, &channel, &stored);
            errors.push(format!("{f}: {e}"));
            continue;
        }
        let id = item.id.clone();
        h.modify(move |d| {
            // the id may have been taken meanwhile
            if d.library.contains(&item.id) {
                item.id = d.library.unique_id(&item.id);
            }
            d.library.upsert(item);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;
        added.push(id);
    }
    Ok(json!({ "added": added, "errors": errors }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_files_are_recognised() {
        assert!(is_own_file("main", "channels/main/media/0a1b.mp4"));
        assert!(!is_own_file("main", "channels/main2/media/0a1b.mp4"));
        assert!(!is_own_file("main", "media/a.mp4"));
        let n = storage_name(Path::new("C:/x/Фильм.MKV"));
        assert!(n.ends_with(".mkv") && n.len() == 36, "{n}");
    }
}

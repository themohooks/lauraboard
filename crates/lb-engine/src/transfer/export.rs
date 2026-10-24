//! Export: a snapshot of the channel's documents (reduced to the chosen
//! parts) plus its media files, written as a ZIP with byte progress.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

use lb_core::config::ChannelConfig;
use lb_core::media::is_url;
use lb_core::project::rebase::channel_prefix;
use lb_core::project::{ChannelData, paths};

use super::{CHANNEL_DIR, FORMAT, MANIFEST, Manifest, Parts, VERSION};
use crate::jobs::JobCtl;
use crate::library::storage::MEDIA_DIR;

/// A file to put into the archive.
struct Item {
    src: PathBuf,
    name: String,
    /// Already-compressed media are stored, not deflated.
    store: bool,
}

/// Drop what was not selected and gather the media files; paths of files
/// from outside the channel folder are pointed into its `media/`.
fn prepare(mut data: ChannelData, root: &Path, parts: &Parts) -> (ChannelData, Vec<Item>) {
    let id = data.id().to_string();
    if !parts.settings {
        let name = data.config.name.clone();
        data.config = ChannelConfig::new(&id);
        data.config.name = name;
    } else if !parts.outputs {
        data.config.outputs.clear();
    }
    if !parts.library {
        data.library = Default::default();
    }
    if !parts.playlists {
        data.playlists.clear();
    }
    if !parts.schedules {
        data.schedules = Default::default();
    }
    if !parts.rules {
        data.rules = Default::default();
    }
    if !parts.graphics {
        data.graphics = Default::default();
    }
    let mut files = Vec::new();
    if parts.media {
        let own = channel_prefix(&id);
        let mut external: HashMap<PathBuf, String> = HashMap::new();
        data.for_each_path_mut(|p| {
            if is_url(p) || p.trim().is_empty() {
                return;
            }
            let abs = paths::resolve(root, p);
            if !abs.is_file() {
                return;
            }
            let stored = p.replace('\\', "/");
            if let Some(rest) = stored.strip_prefix(&own) {
                let name = format!("{CHANNEL_DIR}{rest}");
                if !files.iter().any(|f: &Item| f.name == name) {
                    files.push(Item { src: abs, name, store: true });
                }
                return;
            }
            let file = external.entry(abs.clone()).or_insert_with(|| {
                let ext = abs.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
                let file = format!("{}{ext}", uuid::Uuid::new_v4().simple());
                files.push(Item { src: abs.clone(), name: format!("{CHANNEL_DIR}{MEDIA_DIR}/{file}"), store: true });
                file
            });
            *p = format!("{own}{MEDIA_DIR}/{file}");
        });
    }
    if parts.graphics {
        // the channel's fonts, Google Fonts families among them: styles
        // name those by family, not by path
        let mut fonts = Vec::new();
        collect(&data.dir, &data.dir.join("fonts"), &mut fonts);
        for f in fonts {
            if !files.iter().any(|x: &Item| x.name == f.name) {
                files.push(f);
            }
        }
    }
    // the avatar belongs to the channel's settings
    let avatar = crate::channel::avatar::path(&data.dir);
    if parts.settings && avatar.is_file() {
        files.push(Item { src: avatar, name: format!("{CHANNEL_DIR}{}", crate::channel::avatar::FILE), store: true });
    }
    if parts.history {
        let dir = data.asrun_dir();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten().filter(|e| e.path().is_file()) {
                files.push(Item { src: e.path(), name: format!("{CHANNEL_DIR}asrun/{}", e.file_name().to_string_lossy()), store: false });
            }
        }
    }
    (data, files)
}

/// Job body (blocking): write the archive to `out`.
pub fn run(data: ChannelData, root: &Path, out: &Path, parts: Parts, ctl: &JobCtl) -> Result<Value, String> {
    let id = data.id().to_string();
    let name = data.config.name.clone();
    let (data, media) = prepare(data, root, &parts);
    // documents: let the regular save write them into a scratch folder
    let scratch = std::env::temp_dir().join(format!("laurabrod-export-{}", uuid::Uuid::new_v4().simple()));
    let mut docs = data;
    docs.dir = scratch.clone();
    let result = (|| {
        docs.save().map_err(|e| e.to_string())?;
        let mut items: Vec<Item> = Vec::new();
        collect(&scratch, &scratch, &mut items);
        let media_bytes: u64 = media.iter().filter(|i| i.store).map(|i| size(&i.src)).sum();
        let media_files = media.iter().filter(|i| i.store).count();
        items.extend(media);
        let manifest = Manifest {
            format: FORMAT.into(),
            version: VERSION,
            channel: id.clone(),
            name: name.clone(),
            exported_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            parts,
            media_files,
            media_bytes,
        };
        write_zip(out, &manifest, &items, ctl)?;
        Ok(json!({ "path": out.to_string_lossy(), "files": items.len(), "media_files": media_files, "bytes": size(out) }))
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

fn size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Documents written by `save()` (runtime folders are not created there).
fn collect(base: &Path, dir: &Path, out: &mut Vec<Item>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(base, &p, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push(Item { src: p.clone(), name: format!("{CHANNEL_DIR}{rel}"), store: false });
        }
    }
}

fn write_zip(out: &Path, manifest: &Manifest, items: &[Item], ctl: &JobCtl) -> Result<(), String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let file = File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
    zip.start_file(MANIFEST, deflate).map_err(|e| e.to_string())?;
    zip.write_all(serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string())?;
    let total = items.iter().map(|i| size(&i.src)).sum::<u64>().max(1);
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    for (n, it) in items.iter().enumerate() {
        ctl.check()?;
        let label = it.name.trim_start_matches(CHANNEL_DIR).to_string();
        ctl.progress(done as f32 / total as f32, format!("{}/{}: {label}", n + 1, items.len()));
        zip.start_file(it.name.as_str(), if it.store { stored } else { deflate }).map_err(|e| e.to_string())?;
        let mut src = File::open(&it.src).map_err(|e| format!("{}: {e}", it.src.display()))?;
        let mut last = std::time::Instant::now();
        loop {
            ctl.check()?;
            let k = src.read(&mut buf).map_err(|e| format!("{}: {e}", it.src.display()))?;
            if k == 0 {
                break;
            }
            zip.write_all(&buf[..k]).map_err(|e| format!("{}: {e}", out.display()))?;
            done += k as u64;
            if last.elapsed().as_millis() >= 200 {
                last = std::time::Instant::now();
                ctl.progress(done as f32 / total as f32, format!("{}/{}: {label}", n + 1, items.len()));
            }
        }
    }
    zip.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string())?;
    Ok(())
}

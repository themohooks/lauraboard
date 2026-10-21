//! Media library operations needing IO: importing files (with probing),
//! re-probing and availability scans.

use std::path::{Path, PathBuf};

use lb_core::Dur;
use lb_core::media::{MediaItem, MediaKind};
use lb_core::project::{ChannelData, paths};

const MEDIA_EXT: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "ts", "m2ts", "mxf", "webm", "flv", "mpg", "mpeg", "wmv", "m4v", "mp3", "wav", "flac", "aac", "m4a", "ogg",
];
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp"];

/// Options for importing media.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub kind: Option<MediaKind>,
    pub category: Option<String>,
    pub recursive: bool,
    /// Duration for still images.
    pub image_duration: Option<Dur>,
}

/// Collect media files from paths (files or directories).
pub fn collect_files(inputs: &[PathBuf], recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in inputs {
        if p.is_dir() {
            walk(p, recursive, &mut out);
        } else {
            out.push(p.clone());
        }
    }
    out.sort();
    out
}

fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if recursive {
                walk(&p, recursive, out);
            }
        } else if is_media(&p) {
            out.push(p);
        }
    }
}

fn ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

pub fn is_media(p: &Path) -> bool {
    let e = ext(p);
    MEDIA_EXT.contains(&e.as_str()) || IMAGE_EXT.contains(&e.as_str())
}

/// Import files or URLs into the library, probing each. Returns ids added
/// and errors per input.
pub async fn import(
    data: &mut ChannelData,
    root: &Path,
    ffprobe: &str,
    inputs: &[String],
    opt: &ImportOptions,
) -> (Vec<String>, Vec<String>) {
    let mut added = Vec::new();
    let mut errors = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for i in inputs {
        if lb_core::media::is_url(i) {
            files.push(i.clone());
        } else {
            files.extend(collect_files(&[PathBuf::from(i)], opt.recursive).into_iter().map(|p| p.to_string_lossy().into_owned()));
        }
    }
    for f in files {
        let is_url = lb_core::media::is_url(&f);
        let stored = if is_url { f.clone() } else { paths::to_stored(root, Path::new(&f)) };
        if data.library.items.iter().any(|m| m.path == stored) {
            errors.push(format!("{f}: already in library"));
            continue;
        }
        let stem = if is_url {
            f.rsplit('/').next().unwrap_or("stream").to_string()
        } else {
            Path::new(&f).file_stem().and_then(|s| s.to_str()).unwrap_or("item").to_string()
        };
        let id = data.library.unique_id(&stem);
        let is_image = !is_url && IMAGE_EXT.contains(&ext(Path::new(&f)).as_str());
        let kind = opt.kind.unwrap_or(if is_image {
            MediaKind::Image
        } else if is_url {
            MediaKind::Stream
        } else {
            MediaKind::Program
        });
        let mut item = MediaItem::new(&id, &stored, kind, Dur::ZERO);
        item.title = stem.replace(['_', '.'], " ");
        item.category = opt.category.clone();
        if is_image {
            item.duration = opt.image_duration.unwrap_or(Dur::secs(10));
        } else {
            let resolved = paths::resolve_str(root, &stored);
            match lb_media::probe::probe(ffprobe, &resolved).await {
                Ok(p) => {
                    item.duration = Dur(p.duration_ms);
                    item.probe = p.info;
                }
                Err(e) => {
                    if !is_url {
                        errors.push(format!("{f}: {e}"));
                        continue;
                    }
                    item.probe.error = None; // live streams may not probe offline
                }
            }
        }
        data.library.upsert(item);
        added.push(id);
    }
    (added, errors)
}

/// Re-probe all library items (detects deleted / damaged files).
pub async fn reprobe_all(data: &mut ChannelData, root: &Path, ffprobe: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let ids: Vec<(String, String, bool)> =
        data.library.items.iter().map(|m| (m.id.clone(), m.path.clone(), m.kind == MediaKind::Image || m.is_url())).collect();
    for (id, path, skip) in ids {
        if skip {
            continue;
        }
        let res = lb_media::probe::probe(ffprobe, &paths::resolve_str(root, &path)).await;
        if let Some(m) = data.library.get_mut(&id) {
            match res {
                Ok(p) => {
                    m.duration = Dur(p.duration_ms);
                    m.probe = p.info;
                }
                Err(e) => {
                    m.probe.error = Some(e.to_string());
                    problems.push(format!("{id}: {e}"));
                }
            }
        }
    }
    problems
}

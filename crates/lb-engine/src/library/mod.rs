//! Media library operations needing IO: importing files (with probing),
//! re-probing and availability scans.
//!
//! * [`storage`] — copying media into the channel's own folder
//! * [`backgrounds`] — ready-made backgrounds and video loops of a channel

pub mod backgrounds;
pub mod storage;

use std::path::{Path, PathBuf};

use lb_core::Dur;
use lb_core::media::{MediaItem, MediaKind};
use lb_core::project::{ChannelData, paths};

const MEDIA_EXT: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "ts", "m2ts", "mxf", "webm", "flv", "mpg", "mpeg", "wmv", "m4v", "mp3", "wav", "flac", "aac", "m4a", "ogg",
];
pub(crate) const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp"];

/// Options for importing media.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub kind: Option<MediaKind>,
    pub category: Option<String>,
    pub recursive: bool,
    /// Duration for still images.
    pub image_duration: Option<Dur>,
    /// Library folder for the new items.
    pub folder: String,
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

pub(crate) fn ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

pub fn is_media(p: &Path) -> bool {
    let e = ext(p);
    MEDIA_EXT.contains(&e.as_str()) || IMAGE_EXT.contains(&e.as_str())
}

/// URLs as given, directories expanded to their media files.
pub fn expand_inputs(inputs: &[String], recursive: bool) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for i in inputs {
        if lb_core::media::is_url(i) {
            files.push(i.clone());
        } else {
            files.extend(collect_files(&[PathBuf::from(i)], recursive).into_iter().map(|p| p.to_string_lossy().into_owned()));
        }
    }
    files
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
    for f in expand_inputs(inputs, opt.recursive) {
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
        let mut item = new_item(&data.library, &stored, &stem, is_url, IMAGE_EXT.contains(&ext(Path::new(&f)).as_str()), opt);
        if let Err(e) = probe_item(&mut item, root, ffprobe).await {
            errors.push(format!("{f}: {e}"));
            continue;
        }
        added.push(item.id.clone());
        data.library.upsert(item);
    }
    (added, errors)
}

/// A library item for `stored` (not probed yet).
pub fn new_item(lib: &lb_core::media::Library, stored: &str, stem: &str, is_url: bool, is_image: bool, opt: &ImportOptions) -> MediaItem {
    let is_image = is_image && !is_url;
    let kind = opt.kind.unwrap_or(if is_image {
        MediaKind::Image
    } else if is_url {
        MediaKind::Stream
    } else {
        MediaKind::Program
    });
    let mut item = MediaItem::new(lib.unique_id(stem), stored, kind, Dur::ZERO);
    item.title = stem.replace(['_', '.'], " ");
    item.category = opt.category.clone();
    item.folder = lb_core::media::folders::normalize(&opt.folder);
    if is_image {
        item.duration = opt.image_duration.unwrap_or(Dur::secs(10));
    }
    item
}

/// Fill duration and stream info. Images need no probing; a stream that
/// cannot be probed now is still accepted (it may be offline).
pub async fn probe_item(item: &mut MediaItem, root: &Path, ffprobe: &str) -> Result<(), String> {
    if (item.kind == MediaKind::Image && !item.is_url()) || item.is_scene() {
        return Ok(());
    }
    match lb_media::probe::probe(ffprobe, &paths::resolve_str(root, &item.path)).await {
        Ok(p) => {
            item.duration = Dur(p.duration_ms);
            item.probe = p.info;
            Ok(())
        }
        Err(_) if item.is_url() => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Re-probe all library items (detects deleted / damaged files).
pub async fn reprobe_all(data: &mut ChannelData, root: &Path, ffprobe: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let ids: Vec<(String, String, bool)> = data
        .library
        .items
        .iter()
        .map(|m| (m.id.clone(), m.path.clone(), m.kind == MediaKind::Image || m.is_url() || m.is_scene()))
        .collect();
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

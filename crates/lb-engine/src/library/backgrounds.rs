//! Ready-made backgrounds that come with every channel: still pictures and
//! seamless video loops in the library (folder «Фоны»), stored in
//! `channels/<id>/media/backgrounds/`, plus plate textures in
//! `channels/<id>/graphics/backgrounds/` for shapes and plates.
//!
//! Generating is idempotent: files and items that are there stay, so the
//! job can run again to bring back deleted ones.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use lb_core::Dur;
use lb_core::media::{MediaItem, MediaKind};
use lb_core::project::paths;
use lb_gfx::backdrops::{plates, stills};
use lb_media::args::loops::{self, LOOPS};

use super::probe_item;
use super::storage::MEDIA_DIR;
use crate::channel::ChannelHandle;
use crate::jobs::JobCtl;

/// Sub-folder of the channel media storage.
pub const DIR: &str = "backgrounds";
/// Library folders of the items.
pub const STILLS_FOLDER: &str = "Фоны/Картинки";
pub const LOOPS_FOLDER: &str = "Фоны/Видео";
/// Plate textures: `channels/<id>/graphics/backgrounds/`.
pub const PLATES_DIR: &str = "graphics/backgrounds";

/// What to make.
#[derive(Debug, Clone, Copy)]
pub struct Parts {
    pub stills: bool,
    pub loops: bool,
    pub plates: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Parts { stills: true, loops: true, plates: true }
    }
}

/// Plate textures of a channel folder, written if missing. Returns the
/// stored paths with titles.
pub fn ensure_plates(channel_dir: &Path, root: &Path) -> Result<Vec<(String, String)>, String> {
    let dir = channel_dir.join(PLATES_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for b in plates::PLATES {
        let path = dir.join(format!("{}.png", b.stem));
        if !path.exists() {
            let p = plates::paint(b.stem).ok_or_else(|| format!("plate texture {} failed", b.stem))?;
            p.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        out.push((paths::to_stored(root, &path), b.title.to_string()));
    }
    Ok(out)
}

/// Start the job for `h`; returns its id.
pub fn spawn(engine: &crate::Engine, h: ChannelHandle, parts: Parts) -> u64 {
    let ffmpeg = engine.app.ffmpeg.ffmpeg.clone();
    let ffprobe = engine.app.ffmpeg.ffprobe.clone();
    let id = h.id().to_string();
    engine.jobs.spawn("backgrounds", "Готовые фоны и видеоряды", Some(&id), move |ctl| generate(h, ffmpeg, ffprobe, parts, ctl))
}

/// The channel never had its backgrounds made (new or older channel).
pub fn missing(h: &ChannelHandle) -> bool {
    !h.read(|d| d.dir.join(MEDIA_DIR).join(DIR)).exists()
}

/// Job body: make the backgrounds of channel `h`.
pub async fn generate(h: ChannelHandle, ffmpeg: String, ffprobe: String, parts: Parts, ctl: JobCtl) -> Result<Value, String> {
    let root = h.shared.root.clone();
    let (channel_dir, w, ht, fps) = h.read(|d| (d.dir.clone(), d.config.video.width, d.config.video.height, d.config.video.fps));
    let dir = channel_dir.join(MEDIA_DIR).join(DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let total = (if parts.stills { stills::STILLS.len() } else { 0 } + if parts.loops { LOOPS.len() } else { 0 } + usize::from(parts.plates)).max(1);
    let mut step = 0;
    let mut added: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    if parts.plates {
        ctl.progress(0.0, "Фоны для плашек");
        let (cd, r) = (channel_dir.clone(), root.clone());
        match tokio::task::spawn_blocking(move || ensure_plates(&cd, &r)).await.map_err(|e| e.to_string())? {
            Ok(_) => {}
            Err(e) => errors.push(e),
        }
        step += 1;
    }
    if parts.stills {
        for b in stills::STILLS {
            ctl.check()?;
            ctl.progress(step as f32 / total as f32, format!("Картинка «{}»", b.title));
            let path = dir.join(format!("{}.png", b.stem));
            if !path.exists() {
                let (p2, stem) = (path.clone(), b.stem);
                let made = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let img = stills::paint(stem, w, ht).ok_or("unknown background")?;
                    img.save_png(&p2).map_err(|e| format!("{}: {e}", p2.display()))
                })
                .await
                .map_err(|e| e.to_string())?;
                if let Err(e) = made {
                    errors.push(e);
                    step += 1;
                    continue;
                }
            }
            let mut item = MediaItem::new(b.stem, paths::to_stored(&root, &path), MediaKind::Image, Dur::secs(10));
            item.title = format!("Фон: {}", b.title);
            item.folder = STILLS_FOLDER.into();
            item.category = Some("backgrounds".into());
            if register(&h, item).await? {
                added.push(b.stem.to_string());
            }
            step += 1;
        }
    }
    if parts.loops {
        for l in LOOPS {
            ctl.check()?;
            ctl.progress(step as f32 / total as f32, format!("Видеоряд «{}»", l.title));
            let path = dir.join(format!("{}.mp4", l.stem));
            if !path.exists() {
                let tmp = dir.join(format!("{}.part.mp4", l.stem));
                let args = loops::loop_args(l, w, ht, fps, &tmp.to_string_lossy());
                let out = lb_media::process::command(&ffmpeg).args(&args).output().await.map_err(|e| format!("ffmpeg: {e}"))?;
                if !out.status.success() || !tmp.exists() {
                    let _ = std::fs::remove_file(&tmp);
                    errors.push(format!("{}: {}", l.title, String::from_utf8_lossy(&out.stderr).trim()));
                    step += 1;
                    continue;
                }
                std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            let mut item = MediaItem::new(l.stem, paths::to_stored(&root, &path), MediaKind::Filler, Dur::secs(loops::PERIOD_S as i64));
            item.title = format!("Видеофон: {}", l.title);
            item.folder = LOOPS_FOLDER.into();
            item.category = Some("backgrounds".into());
            if let Err(e) = probe_item(&mut item, &root, &ffprobe).await {
                errors.push(format!("{}: {e}", l.title));
                step += 1;
                continue;
            }
            if register(&h, item).await? {
                added.push(l.stem.to_string());
            }
            step += 1;
        }
    }
    ctl.progress(1.0, "Готово");
    Ok(json!({ "added": added, "errors": errors }))
}

/// Add the item unless the library has one for the same file. True if
/// added.
async fn register(h: &ChannelHandle, mut item: MediaItem) -> Result<bool, String> {
    let exists = h.read(|d| d.library.items.iter().any(|m| m.path.replace('\\', "/") == item.path.replace('\\', "/")));
    if exists {
        return Ok(false);
    }
    h.modify(move |d| {
        if d.library.contains(&item.id) {
            item.id = d.library.unique_id(&item.id);
        }
        d.library.upsert(item);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Plate texture files (stored paths) of a channel, with titles.
pub fn plate_files(channel_dir: &Path, root: &Path) -> Vec<(String, String)> {
    let dir: PathBuf = channel_dir.join(PLATES_DIR);
    plates::PLATES
        .iter()
        .filter_map(|b| {
            let p = dir.join(format!("{}.png", b.stem));
            p.exists().then(|| (paths::to_stored(root, &p), b.title.to_string()))
        })
        .collect()
}

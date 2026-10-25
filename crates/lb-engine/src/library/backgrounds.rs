//! Ready-made backgrounds that come with every channel: still pictures and
//! seamless video loops in the library (folder «Фоны»), stored in
//! `channels/<id>/media/backgrounds/`, plus plate textures in
//! `channels/<id>/graphics/backgrounds/` for shapes and plates.
//!
//! The files are part of the program (rendered once by `cargo xtask
//! backgrounds` into `assets/backgrounds/` and embedded here): a channel
//! only gets copies, nothing is generated on the server. Installing is
//! idempotent: files and items that are there stay.

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

macro_rules! embed {
    ($dir:literal, $ext:literal: $($stem:literal),* $(,)?) => {
        &[$(($stem, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/backgrounds/", $dir, "/", $stem, ".", $ext)) as &[u8])),*]
    };
}

/// Embedded files: (stem, bytes).
type Files = &'static [(&'static str, &'static [u8])];

const STILL_FILES: Files = embed!("stills", "jpg":
    "bg-midnight", "bg-sunset", "bg-ocean", "bg-forest", "bg-graphite", "bg-aurora", "bg-bokeh", "bg-dots",
    "bg-waves", "bg-studio", "bg-news", "bg-pastel", "bg-ember", "bg-violet", "bg-lines", "bg-cosmos",
);
const LOOP_FILES: Files = embed!("loops", "mp4":
    "loop-plasma-blue", "loop-plasma-violet", "loop-flow-sunset", "loop-flow-sea", "loop-waves",
    "loop-aurora", "loop-pulse", "loop-grid", "loop-ember", "loop-mint",
);
const PLATE_FILES: Files = embed!("plates", "png":
    "plate-blue-gloss", "plate-red", "plate-carbon", "plate-glass", "plate-gold", "plate-neon",
    "plate-stripes", "plate-dusk", "plate-emerald", "plate-steel", "plate-sunrise", "plate-night",
    "plate-ocean", "plate-sky", "plate-fire", "plate-violet-gloss", "plate-mint", "plate-silver",
    "plate-chevrons", "plate-halftone", "plate-paper", "plate-newsroom", "plate-aurora", "plate-holo",
    "plate-rose", "plate-grid", "plate-split", "plate-cyber",
);

fn bytes(files: Files, stem: &str) -> Option<&'static [u8]> {
    files.iter().find(|(s, _)| *s == stem).map(|(_, b)| *b)
}

/// Write `data` to `path` unless the file is there.
fn put(path: &Path, data: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))
}

/// Plate textures of a channel folder, written if missing. Returns the
/// stored paths with titles.
pub fn ensure_plates(channel_dir: &Path, root: &Path) -> Result<Vec<(String, String)>, String> {
    let dir = channel_dir.join(PLATES_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for b in plates::PLATES {
        let Some(data) = bytes(PLATE_FILES, b.stem) else { continue };
        let path = dir.join(format!("{}.png", b.stem));
        put(&path, data)?;
        out.push((paths::to_stored(root, &path), b.title.to_string()));
    }
    Ok(out)
}

/// Start installing the backgrounds of `h` (a short job: copies and a
/// probe of the loops); returns its id.
pub fn spawn(engine: &crate::Engine, h: ChannelHandle) -> u64 {
    let ffprobe = engine.app.ffmpeg.ffprobe.clone();
    let id = h.id().to_string();
    engine.jobs.spawn("backgrounds", "Готовые фоны и видеоряды", Some(&id), move |ctl| install(h, ffprobe, ctl))
}

/// The channel never got its backgrounds (new or older channel).
pub fn missing(h: &ChannelHandle) -> bool {
    !h.read(|d| d.dir.join(MEDIA_DIR).join(DIR)).exists()
}

/// Job body: copy the backgrounds into channel `h` and add them to its
/// library.
pub async fn install(h: ChannelHandle, ffprobe: String, ctl: JobCtl) -> Result<Value, String> {
    let root = h.shared.root.clone();
    let channel_dir = h.read(|d| d.dir.clone());
    let dir = channel_dir.join(MEDIA_DIR).join(DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let total = (stills::STILLS.len() + LOOPS.len() + 1) as f32;
    let mut step = 0.0;
    let mut added: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    ctl.progress(0.0, "Фоны для плашек");
    if let Err(e) = ensure_plates(&channel_dir, &root) {
        errors.push(e);
    }
    step += 1.0;
    for b in stills::STILLS {
        ctl.check()?;
        ctl.progress(step / total, format!("Картинка «{}»", b.title));
        step += 1.0;
        let Some(data) = bytes(STILL_FILES, b.stem) else { continue };
        let path = dir.join(format!("{}.jpg", b.stem));
        if let Err(e) = put(&path, data) {
            errors.push(e);
            continue;
        }
        let mut item = MediaItem::new(b.stem, paths::to_stored(&root, &path), MediaKind::Image, Dur::secs(10));
        item.title = format!("Фон: {}", b.title);
        item.folder = STILLS_FOLDER.into();
        item.category = Some("backgrounds".into());
        if register(&h, item).await? {
            added.push(b.stem.to_string());
        }
    }
    for l in LOOPS {
        ctl.check()?;
        ctl.progress(step / total, format!("Видеоряд «{}»", l.title));
        step += 1.0;
        let Some(data) = bytes(LOOP_FILES, l.stem) else { continue };
        let path: PathBuf = dir.join(format!("{}.mp4", l.stem));
        if let Err(e) = put(&path, data) {
            errors.push(e);
            continue;
        }
        let mut item = MediaItem::new(l.stem, paths::to_stored(&root, &path), MediaKind::Filler, Dur::secs(loops::PERIOD_S as i64));
        item.title = format!("Видеофон: {}", l.title);
        item.folder = LOOPS_FOLDER.into();
        item.category = Some("backgrounds".into());
        if let Err(e) = probe_item(&mut item, &root, &ffprobe).await {
            // the file is fine (it ships with the program): keep the item
            // with the known length, the next check probes it again
            tracing::warn!(item = l.stem, "probe of a ready-made loop failed: {e}");
        }
        if register(&h, item).await? {
            added.push(l.stem.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every listed background has its embedded file (`cargo xtask
    /// backgrounds` renders new ones).
    #[test]
    fn every_background_is_embedded() {
        for b in stills::STILLS {
            assert!(bytes(STILL_FILES, b.stem).is_some_and(|d| d.starts_with(&[0xFF, 0xD8])), "{}", b.stem);
        }
        for l in LOOPS {
            assert!(bytes(LOOP_FILES, l.stem).is_some_and(|d| d.len() > 10_000), "{}", l.stem);
        }
        for p in plates::PLATES {
            assert!(bytes(PLATE_FILES, p.stem).is_some_and(|d| d.starts_with(b"\x89PNG")), "{}", p.stem);
        }
        assert_eq!((STILL_FILES.len(), LOOP_FILES.len(), PLATE_FILES.len()), (stills::STILLS.len(), LOOPS.len(), plates::PLATES.len()));
    }
}

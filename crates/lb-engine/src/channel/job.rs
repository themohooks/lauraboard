//! Turning a rundown item into a concrete decoder command line.

use std::path::Path;

use lb_core::media::{MediaItem, MediaKind};
use lb_core::project::{ChannelData, paths};
use lb_core::rundown::RundownItem;
use lb_media::args::decoder::{DecodeJob, InputKind, decoder_args};
use lb_media::args::scene::{SceneJob, SceneTrack, SceneVideo, scene_args};
use lb_media::args::slate::slate_args;

/// Why an item cannot be played.
#[derive(Debug, Clone, PartialEq)]
pub enum JobError {
    UnknownMedia(String),
    MissingFile(String),
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobError::UnknownMedia(m) => write!(f, "media '{m}' is not in the library"),
            JobError::MissingFile(p) => write!(f, "file not found: {p}"),
        }
    }
}

fn input_kind(m: &MediaItem, resolved: &str) -> InputKind {
    let l = resolved.to_ascii_lowercase();
    if m.kind == MediaKind::Image || [".png", ".jpg", ".jpeg", ".bmp", ".webp", ".gif"].iter().any(|e| l.ends_with(e)) {
        InputKind::Image
    } else if m.is_url() || m.kind == MediaKind::Stream {
        InputKind::Stream
    } else {
        InputKind::File
    }
}

/// Decoder (or slate) arguments for `item`, starting `offset_ms` into it.
pub fn build(data: &ChannelData, root: &Path, item: &RundownItem, offset_ms: i64, loglevel: &str) -> Result<Vec<String>, JobError> {
    let cfg = &data.config;
    let remaining = (item.duration_ms - offset_ms).max(500);
    if item.media.is_empty() {
        let img = cfg.fallback.image.as_deref().map(|p| paths::resolve_str(root, p)).filter(|p| Path::new(p).exists());
        return Ok(slate_args(img.as_deref(), remaining, &cfg.video, &cfg.audio, loglevel));
    }
    let m = data.library.get(&item.media).ok_or_else(|| JobError::UnknownMedia(item.media.clone()))?;
    if let Some(scene) = &m.scene {
        return scene_job(data, root, item, m, scene, offset_ms, remaining, loglevel);
    }
    let input = paths::resolve_str(root, &m.path);
    let kind = input_kind(m, &input);
    if kind != InputKind::Stream && !Path::new(&input).exists() {
        return Err(JobError::MissingFile(input));
    }
    let fade_out = if offset_ms + remaining >= item.duration_ms { item.fade_out_ms } else { 0 };
    let job = DecodeJob {
        input,
        kind,
        seek_ms: item.seek_ms + offset_ms,
        duration_ms: remaining,
        has_audio: m.probe.has_audio,
        volume_db: m.volume_db,
        fade_in_ms: if offset_ms == 0 { item.fade_in_ms } else { 0 },
        fade_out_ms: fade_out,
        display: m.display.clone(),
    };
    Ok(decoder_args(&job, &cfg.video, &cfg.audio, loglevel))
}

/// A scene: its background and the music of this airing.
#[allow(clippy::too_many_arguments)]
fn scene_job(
    data: &ChannelData,
    root: &Path,
    item: &RundownItem,
    m: &MediaItem,
    scene: &lb_core::media::Scene,
    offset_ms: i64,
    remaining: i64,
    loglevel: &str,
) -> Result<Vec<String>, JobError> {
    use lb_core::media::scene::Background;
    let cfg = &data.config;
    let file = |id: &str| -> Result<(&MediaItem, String), JobError> {
        let bg = data.library.get(id).ok_or_else(|| JobError::UnknownMedia(id.to_string()))?;
        let p = paths::resolve_str(root, &bg.path);
        if !Path::new(&p).exists() {
            return Err(JobError::MissingFile(p));
        }
        Ok((bg, p))
    };
    let (video, display) = match &scene.background {
        Background::Color { color } => (SceneVideo::Color(color.clone()), m.display.clone()),
        Background::Media { media } => {
            let (bg, p) = file(media)?;
            let v = if input_kind(bg, &p) == InputKind::Image { SceneVideo::Image(p) } else { SceneVideo::Video { input: p, length_ms: bg.duration.ms() } };
            (v, bg.display.clone())
        }
    };
    // the whole airing's music, so a restart / rewind continues it
    let plan = scene.music_plan(&data.library, item.start, &item.uid, offset_ms, remaining);
    let tracks = plan
        .cues
        .iter()
        .filter_map(|c| {
            let t = data.library.get(&c.media)?;
            let p = paths::resolve_str(root, &t.path);
            Path::new(&p).exists().then_some(SceneTrack { input: p, seek_ms: c.seek_ms, play_ms: c.play_ms })
        })
        .collect();
    let fade_out = if offset_ms + remaining >= item.duration_ms { item.fade_out_ms } else { 0 };
    let job = SceneJob {
        video,
        display,
        tracks,
        music_volume_db: plan.volume_db,
        music_fade_in_ms: plan.fade_in_ms,
        music_fade_out_ms: plan.fade_out_ms,
        offset_ms,
        duration_ms: remaining,
        volume_db: m.volume_db,
        fade_in_ms: if offset_ms == 0 { item.fade_in_ms } else { 0 },
        fade_out_ms: fade_out,
    };
    Ok(scene_args(&job, &cfg.video, &cfg.audio, loglevel))
}

/// Whether the media needs probing before it can be played safely.
pub fn needs_probe(m: &MediaItem) -> bool {
    m.scene.is_none() && m.probe.video.is_none() && !m.probe.has_audio && m.probe.error.is_none() && m.kind != MediaKind::Image
}

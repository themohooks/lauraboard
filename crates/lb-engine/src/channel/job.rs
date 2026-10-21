//! Turning a rundown item into a concrete decoder command line.

use std::path::Path;

use lb_core::media::{MediaItem, MediaKind};
use lb_core::project::{ChannelData, paths};
use lb_core::rundown::RundownItem;
use lb_media::args::decoder::{DecodeJob, InputKind, decoder_args};
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
    };
    Ok(decoder_args(&job, &cfg.video, &cfg.audio, loglevel))
}

/// Whether the media needs probing before it can be played safely.
pub fn needs_probe(m: &MediaItem) -> bool {
    m.probe.video.is_none() && !m.probe.has_audio && m.probe.error.is_none() && m.kind != MediaKind::Image
}

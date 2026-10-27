//! Video editor job: render a library video with an [`EditSpec`] into the
//! channel's storage, as a new item next to the original or in place of
//! the original's copy (same id, so playlists keep working).

use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

use lb_core::media::edit::EditSpec;
use lb_core::media::{MediaItem, MediaKind};
use lb_core::project::paths;
use lb_media::args::edit::{Source, render_args};
use lb_media::progress::ProgressParser;

use crate::channel::ChannelHandle;
use crate::jobs::JobCtl;
use crate::library::probe_item;
use crate::library::storage::{MEDIA_DIR, is_own_file, remove_own_file};

/// What to render.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub media: String,
    pub spec: EditSpec,
    /// Replace the original (only for files in the channel's storage).
    #[serde(default)]
    pub replace: bool,
    /// Title of the new item (default: the original's with a mark).
    #[serde(default)]
    pub title: Option<String>,
}

/// The item can be edited: a video file of the library.
pub fn check(item: &MediaItem, channel: &str, req: &Request) -> Result<(), String> {
    if item.is_url() || item.is_scene() || item.kind == MediaKind::Image || item.kind == MediaKind::Stream {
        return Err("only video files can be edited".into());
    }
    if item.probe.video.is_none() {
        return Err("no video in this media".into());
    }
    if req.replace && !is_own_file(channel, &item.path) {
        return Err("only copies in the channel's storage can be replaced; save as a new item".into());
    }
    req.spec.validate(item.duration.ms())
}

pub fn source(item: &MediaItem) -> Source {
    let v = item.probe.video.clone().unwrap_or_default();
    Source { width: v.width, height: v.height, duration_ms: item.duration.ms(), has_audio: item.probe.has_audio }
}

/// Job body.
pub async fn run(h: ChannelHandle, ffmpeg: String, ffprobe: String, req: Request, ctl: JobCtl) -> Result<Value, String> {
    let root = h.shared.root.clone();
    let channel = h.id().to_string();
    let item = h.read(|d| d.library.get(&req.media).cloned()).ok_or_else(|| format!("unknown media '{}'", req.media))?;
    check(&item, &channel, &req)?;
    let dir = h.read(|d| d.dir.join(MEDIA_DIR));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let out = dir.join(format!("{}.mp4", uuid::Uuid::new_v4().simple()));
    let src = source(&item);
    let (start, end) = req.spec.range(src.duration_ms);
    let input = paths::resolve_str(&root, &item.path);
    let args = render_args(&input, &out.to_string_lossy(), &req.spec, &src);
    ctl.progress(0.0, "обработка видео");
    if let Err(e) = render(&ffmpeg, &args, end - start, &ctl).await {
        let _ = std::fs::remove_file(&out);
        return Err(e);
    }
    let stored = paths::to_stored(&root, &out);
    let mut edited = item.clone();
    edited.path = stored.clone();
    edited.in_point = None;
    edited.out_point = None;
    ctl.progress(0.98, "проверка файла");
    if let Err(e) = probe_item(&mut edited, &root, &ffprobe).await {
        remove_own_file(&root, &channel, &stored);
        return Err(e);
    }
    if req.replace {
        let id = item.id.clone();
        let old = item.path.clone();
        h.modify(move |d| {
            let Some(m) = d.library.get_mut(&id) else {
                return Err(crate::error::EngineError::Core(lb_core::CoreError::NotFound(format!("media '{id}'"))));
            };
            m.path = edited.path;
            m.duration = edited.duration;
            m.probe = edited.probe;
            m.in_point = None;
            m.out_point = None;
            Ok(())
        })
        .await
        .map_err(|e| {
            remove_own_file(&root, &channel, &stored);
            e.to_string()
        })?;
        remove_own_file(&root, &channel, &old);
        return Ok(json!({ "media": item.id, "replaced": true }));
    }
    edited.title = req.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| format!("{} (ред.)", item.title));
    let new_id = h
        .modify(move |d| {
            edited.id = d.library.unique_id(&format!("{}-edit", item.id));
            let id = edited.id.clone();
            d.library.upsert(edited);
            Ok(id)
        })
        .await
        .map_err(|e| {
            remove_own_file(&root, &channel, &stored);
            e.to_string()
        })?;
    Ok(json!({ "media": new_id, "replaced": false }))
}

/// Run ffmpeg, reporting progress of `dur_ms` of output; killed on cancel.
async fn render(ffmpeg: &str, args: &[String], dur_ms: i64, ctl: &JobCtl) -> Result<(), String> {
    let mut cmd = lb_media::process::command(ffmpeg);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("ffmpeg: {e}"))?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let stderr = child.stderr.take().ok_or("no output")?;
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut last = String::new();
        while let Ok(Some(l)) = lines.next_line().await {
            if !l.trim().is_empty() {
                last = l;
            }
        }
        last
    });
    let mut parser = ProgressParser::default();
    let mut lines = BufReader::new(stdout).lines();
    let mut tick = tokio::time::interval(Duration::from_millis(300));
    loop {
        tokio::select! {
            l = lines.next_line() => {
                let Ok(Some(l)) = l else { break };
                if let Some(s) = parser.feed(&l) {
                    let f = if dur_ms > 0 { (s.out_time_ms as f32 / dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
                    let speed = if s.speed > 0.0 { format!(" · {:.1}×", s.speed) } else { String::new() };
                    ctl.progress(f * 0.97, format!("обработка видео{speed}"));
                }
            }
            _ = tick.tick() => {
                if ctl.cancelled() {
                    let _ = child.kill().await;
                    return Err("cancelled".into());
                }
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let last = err_task.await.unwrap_or_default();
    if !status.success() {
        return Err(if last.is_empty() { "ffmpeg failed".into() } else { format!("ffmpeg: {last}") });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::Dur;
    use lb_core::media::VideoInfo;

    fn video(path: &str) -> MediaItem {
        let mut m = MediaItem::new("clip", path, MediaKind::Program, Dur::secs(30));
        m.probe.video = Some(VideoInfo { width: 1280, height: 720, fps: 25.0, codec: "h264".into() });
        m
    }

    fn req(replace: bool) -> Request {
        Request { media: "clip".into(), spec: EditSpec::default(), replace, title: None }
    }

    #[test]
    fn what_can_be_edited() {
        assert!(check(&video("channels/tv/media/a.mp4"), "tv", &req(true)).is_ok());
        assert!(check(&video("D:/films/a.mp4"), "tv", &req(false)).is_ok());
        assert!(check(&video("D:/films/a.mp4"), "tv", &req(true)).is_err());
        assert!(check(&video("https://example.com/live.m3u8"), "tv", &req(false)).is_err());
        let mut audio = video("channels/tv/media/a.mp3");
        audio.probe.video = None;
        assert_eq!(check(&audio, "tv", &req(false)).unwrap_err(), "no video in this media");
    }
}

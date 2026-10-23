//! Media thumbnails: one frame grabbed by ffmpeg at 10 % of the item,
//! cached under `<project>/cache/thumbs/<channel>/`. At most two ffmpeg
//! processes run at a time so browsing the library never competes with the
//! on-air encoder for long.

use std::path::{Path as FsPath, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::sync::Semaphore;

use lb_core::config::Role;
use lb_core::media::{MediaDisplay, MediaItem, MediaKind};
use lb_core::project::paths;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

const PARALLEL: usize = 2;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
pub struct ThumbQuery {
    /// Width in pixels (default 320, 32…1280); height keeps the aspect.
    pub w: Option<u32>,
}

pub async fn thumb(
    State(s): State<AppState>,
    c: Caller,
    Path((id, media)): Path<(String, String)>,
    Query(q): Query<ThumbQuery>,
) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let item = h.read(|d| d.library.get(&media).cloned()).ok_or_else(|| ApiError::not_found(format!("media '{media}'")))?;
    if item.kind == MediaKind::Stream || item.is_url() {
        return Err(ApiError::not_found("no thumbnails for streams"));
    }
    if item.probe.video.is_none() && (item.probe.has_audio || item.probe.error.is_some()) {
        return Err(ApiError::not_found("no video in this media"));
    }
    let root = h.shared.root.clone();
    let src = paths::resolve(&root, &item.path);
    let mtime = std::fs::metadata(&src)
        .and_then(|m| m.modified())
        .map_err(|_| ApiError::not_found("media file not found"))?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let w = q.w.unwrap_or(320).clamp(32, 1280);
    let dir = root.join("cache").join("thumbs").join(file_safe(&id));
    let key = file_safe(&item.id);
    let file = dir.join(format!("{key}-{mtime}-{w}.jpg"));
    if let Ok(b) = tokio::fs::read(&file).await {
        return Ok(jpeg(b));
    }

    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    let _slot = SLOTS.get_or_init(|| Semaphore::new(PARALLEL)).acquire().await.map_err(|_| ApiError::not_found("shutting down"))?;
    // another request may have made it while we waited
    if let Ok(b) = tokio::fs::read(&file).await {
        return Ok(jpeg(b));
    }
    tokio::fs::create_dir_all(&dir).await.map_err(internal)?;
    let tmp = dir.join(format!("{key}-{mtime}-{w}.tmp.jpg"));
    let ffmpeg = &s.engine.app.ffmpeg.ffmpeg;
    let mut ok = grab(ffmpeg, &src, &tmp, seek_ms(&item), w).await;
    if !ok && seek_ms(&item) > 0 {
        // short or oddly indexed files: fall back to the first frame
        ok = grab(ffmpeg, &src, &tmp, 0, w).await;
    }
    let bytes = if ok { tokio::fs::read(&tmp).await.ok().filter(|b| !b.is_empty()) } else { None };
    let Some(bytes) = bytes else {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(ApiError::not_found("could not extract a frame"));
    };
    remove_stale(&dir, &key).await;
    let _ = tokio::fs::rename(&tmp, &file).await;
    Ok(jpeg(bytes))
}

/// One frame of the media as it would look on air with `display` (body):
/// channel size and scaling, crop and picture filters. Not cached.
pub async fn preview(
    State(s): State<AppState>,
    c: Caller,
    Path((id, media)): Path<(String, String)>,
    Query(q): Query<ThumbQuery>,
    axum::Json(display): axum::Json<MediaDisplay>,
) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let (item, video) = h
        .read(|d| d.library.get(&media).cloned().map(|m| (m, d.config.video.clone())))
        .ok_or_else(|| ApiError::not_found(format!("media '{media}'")))?;
    if item.kind == MediaKind::Stream || item.is_url() {
        return Err(ApiError::not_found("no preview for streams"));
    }
    let src = paths::resolve(&h.shared.root, &item.path);
    if !src.exists() {
        return Err(ApiError::not_found("media file not found"));
    }
    let w = q.w.unwrap_or(480).clamp(32, 1280);
    let chain = lb_media::args::filters::video_chain(&video, &display, 0, 0, 0);
    let graph = format!("[0:v:0]{chain},scale={w}:-2[v]");
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    let _slot = SLOTS.get_or_init(|| Semaphore::new(PARALLEL)).acquire().await.map_err(|_| ApiError::not_found("shutting down"))?;
    let mut cmd = lb_media::process::command(&s.engine.app.ffmpeg.ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    let at = seek_ms(&item);
    if at > 0 {
        cmd.args(["-ss", &lb_media::args::secs(at)]);
    }
    cmd.arg("-i").arg(&src);
    cmd.args(["-filter_complex", &graph, "-map", "[v]", "-frames:v", "1", "-an", "-q:v", "4", "-f", "image2", "-c:v", "mjpeg", "pipe:1"]);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let out = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(internal(e)),
        Err(_) => return Err(internal("ffmpeg timed out")),
    };
    if !out.status.success() || out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("could not render a frame").to_string();
        return Err(ApiError::bad_request(err));
    }
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], out.stdout).into_response())
}

fn jpeg(b: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "max-age=300")], b).into_response()
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// 10 % into the played part (in point respected); images have one frame.
fn seek_ms(item: &MediaItem) -> i64 {
    if item.kind == MediaKind::Image {
        return 0;
    }
    item.in_point.map(|d| d.ms()).unwrap_or(0) + item.play_duration() / 10
}

async fn grab(ffmpeg: &str, src: &FsPath, out: &FsPath, at_ms: i64, w: u32) -> bool {
    let mut cmd = lb_media::process::command(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"]);
    if at_ms > 0 {
        cmd.args(["-ss", &lb_media::args::secs(at_ms)]);
    }
    cmd.arg("-i").arg(src);
    cmd.args(["-frames:v", "1", "-an", "-vf", &format!("scale={w}:-2"), "-q:v", "4", "-f", "image2"]).arg(out);
    cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    match tokio::time::timeout(TIMEOUT, cmd.status()).await {
        Ok(Ok(st)) => st.success(),
        _ => false,
    }
}

/// Drop thumbnails of older versions of the file (`<key>-<mtime>-<w>.jpg`).
async fn remove_stale(dir: &FsPath, key: &str) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else { return };
    let prefix = format!("{key}-");
    let mut old: Vec<PathBuf> = Vec::new();
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix(&prefix).and_then(|r| r.strip_suffix(".jpg")) else { continue };
        if rest.split('-').count() == 2 && rest.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
            old.push(e.path());
        }
    }
    for p in old {
        let _ = tokio::fs::remove_file(p).await;
    }
}

/// File-name-safe, collision-free form of an id: anything but `[A-Za-z0-9_]`
/// becomes `~XX` (so `-` stays free as a separator).
fn file_safe(id: &str) -> String {
    let mut s = String::with_capacity(id.len());
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b == b'_' {
            s.push(b as char);
        } else {
            s.push_str(&format!("~{b:02x}"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names_do_not_collide() {
        assert_eq!(file_safe("news_01"), "news_01");
        assert_ne!(file_safe("a.b"), file_safe("a_b"));
        assert_eq!(file_safe("a-b"), "a~2db");
        assert_eq!(file_safe("кот"), "~d0~ba~d0~be~d1~82");
    }
}

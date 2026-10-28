//! Video editor: preview frames with the hidden areas drawn, and the job
//! rendering the edited video into the library.

use std::sync::OnceLock;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use lb_core::config::Role;
use lb_core::media::edit::EditSpec;
use lb_core::project::paths;
use lb_engine::edit;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
pub struct FrameQuery {
    /// Source time in ms.
    #[serde(default)]
    pub at: i64,
    /// Width in pixels (default 960, 64…1920).
    pub w: Option<u32>,
}

/// One frame of the source at `at` with the hidden areas of the body drawn
/// (no crop: the frame keeps the source coordinates). Not cached.
pub async fn frame(
    State(s): State<AppState>,
    c: Caller,
    Path((id, media)): Path<(String, String)>,
    Query(q): Query<FrameQuery>,
    Json(spec): Json<EditSpec>,
) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let item = h.read(|d| d.library.get(&media).cloned()).ok_or_else(|| ApiError::not_found(format!("media '{media}'")))?;
    let req = edit::Request { media: media.clone(), spec: EditSpec::default(), replace: false, title: None };
    edit::check(&item, h.id(), &req).map_err(ApiError::bad_request)?;
    let src = paths::resolve(&h.shared.root, &item.path);
    if !src.exists() {
        return Err(ApiError::not_found("media file not found"));
    }
    let w = q.w.unwrap_or(960).clamp(64, 1920);
    let at = q.at.clamp(0, (item.duration.ms() - 40).max(0));
    let args = lb_media::args::edit::frame_args(&src.to_string_lossy(), &spec, &edit::source(&item), at, w);
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    let _slot = SLOTS.get_or_init(|| Semaphore::new(2)).acquire().await.map_err(|_| ApiError::not_found("shutting down"))?;
    let mut cmd = lb_media::process::command(&s.engine.app.ffmpeg.ffmpeg);
    cmd.args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let out = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(ApiError::bad_request(e.to_string())),
        Err(_) => return Err(ApiError::bad_request("ffmpeg timed out")),
    };
    if !out.status.success() || out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("could not extract a frame").to_string();
        return Err(ApiError::bad_request(err));
    }
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], out.stdout).into_response())
}

#[derive(Deserialize)]
pub struct RenderBody {
    #[serde(default)]
    pub spec: EditSpec,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub title: Option<String>,
}

/// Render the edited video (a background job); the reply is `{"job": id}`.
pub async fn render(
    State(s): State<AppState>,
    c: Caller,
    Path((id, media)): Path<(String, String)>,
    Json(b): Json<RenderBody>,
) -> ApiResult<Json<Value>> {
    c.require(Role::Operator)?;
    let h = channel(&s, &id)?;
    let item = h.read(|d| d.library.get(&media).cloned()).ok_or_else(|| ApiError::not_found(format!("media '{media}'")))?;
    let req = edit::Request { media, spec: b.spec, replace: b.replace, title: b.title };
    edit::check(&item, h.id(), &req).map_err(ApiError::bad_request)?;
    let ffmpeg = s.engine.app.ffmpeg.ffmpeg.clone();
    let ffprobe = s.engine.app.ffmpeg.ffprobe.clone();
    let title = format!("Монтаж: {}", item.title);
    let job = s.engine.jobs.spawn("edit_media", &title, Some(&id), move |ctl| edit::run(h, ffmpeg, ffprobe, req, ctl));
    Ok(Json(json!({ "job": job })))
}

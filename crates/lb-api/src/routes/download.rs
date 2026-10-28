//! Downloading videos from sites with yt-dlp: the tool's state and
//! installation, reading a link, the download job.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use lb_core::config::Role;
use lb_core::media::MediaKind;
use lb_engine::download::info::{self, Entry, LinkInfo};
use lb_engine::download::{job, login, tool};
use lb_engine::library::ImportOptions;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

async fn ytdlp(s: &AppState) -> ApiResult<std::path::PathBuf> {
    lb_engine::download::javascript::prepare(&s.engine.project_root).map_err(ApiError::bad_request)?;
    tool::locate(&s.engine.app.tools.ytdlp, &s.engine.project_root)
        .await
        .map(|f| f.0)
        .ok_or_else(|| ApiError::bad_request("yt-dlp is not installed"))
}

/// Whether yt-dlp is available, its version and whether it can be installed.
pub async fn status(State(s): State<AppState>, c: Caller) -> ApiResult<Json<tool::ToolStatus>> {
    c.require(Role::Viewer)?;
    Ok(Json(tool::status(&s.engine.app.tools.ytdlp, &s.engine.project_root).await))
}

/// Install or update yt-dlp in `<project>/tools` (a background job).
pub async fn install(State(s): State<AppState>, c: Caller) -> ApiResult<Json<Value>> {
    c.require(Role::Admin)?;
    let root = s.engine.project_root.clone();
    let job = s.engine.jobs.spawn("install_ytdlp", "Установка yt-dlp", None, move |ctl| tool::install(root, ctl));
    Ok(Json(json!({ "job": job })))
}

/// How yt-dlp signs in to sites (browser cookies or a cookies.txt).
pub async fn get_login(State(s): State<AppState>, c: Caller) -> ApiResult<Json<login::Login>> {
    c.require(Role::Operator)?;
    Ok(Json(login::load(&s.engine.project_root)))
}

#[derive(Deserialize)]
pub struct LoginBody {
    #[serde(default)]
    pub browser: Option<String>,
    /// Contents of a cookies.txt; "" removes the stored one, absent keeps it.
    #[serde(default)]
    pub cookies: Option<String>,
}

/// Choose how to sign in. Cookies are account access: administrators only.
pub async fn put_login(State(s): State<AppState>, c: Caller, Json(b): Json<LoginBody>) -> ApiResult<Json<login::Login>> {
    c.require(Role::Admin)?;
    login::save(&s.engine.project_root, b.browser.as_deref(), b.cookies.as_deref()).map(Json).map_err(ApiError::bad_request)
}

#[derive(Deserialize)]
pub struct ProbeBody {
    pub url: String,
}

/// What a link offers: title, duration, qualities, playlist entries.
pub async fn probe(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(b): Json<ProbeBody>) -> ApiResult<Json<LinkInfo>> {
    c.require(Role::Operator)?;
    channel(&s, &id)?;
    let exe = ytdlp(&s).await?;
    job::probe(&exe, &login::args(&s.engine.project_root), &b.url).await.map(Json).map_err(ApiError::bad_request)
}

#[derive(Deserialize)]
pub struct DownloadBody {
    /// Videos to download (from the probe; a single link is also accepted).
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub url: Option<String>,
    /// Quality id from the probe (`best`, `h720`, `audio` …).
    #[serde(default = "best")]
    pub quality: String,
    #[serde(default)]
    pub kind: Option<MediaKind>,
    #[serde(default)]
    pub category: Option<String>,
    /// Library folder for the new items.
    #[serde(default)]
    pub folder: String,
}

fn best() -> String {
    "best".into()
}

/// Start downloading into the channel's storage; the reply is `{"job": id}`.
pub async fn download(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(b): Json<DownloadBody>) -> ApiResult<Json<Value>> {
    c.require(Role::Operator)?;
    let h = channel(&s, &id)?;
    let mut entries = b.entries;
    if let Some(u) = b.url.filter(|u| !u.trim().is_empty()) {
        entries.push(Entry { url: u.trim().to_string(), title: String::new(), duration_ms: 0 });
    }
    if entries.is_empty() {
        return Err(ApiError::bad_request("nothing to download"));
    }
    if let Some(bad) = entries.iter().find(|e| !info::is_web_link(&e.url)) {
        return Err(ApiError::bad_request(format!("not a web link: {}", bad.url)));
    }
    if !info::is_quality(&b.quality) {
        return Err(ApiError::bad_request(format!("unknown quality '{}'", b.quality)));
    }
    let tools = job::Tools {
        ytdlp: ytdlp(&s).await?,
        ffmpeg: lb_media::process::bundled_program(&s.engine.app.ffmpeg.ffmpeg),
        ffprobe: s.engine.app.ffmpeg.ffprobe.clone(),
        login: login::args(&s.engine.project_root),
    };
    let title = match entries.as_slice() {
        [one] if !one.title.is_empty() => format!("Загрузка: {}", one.title),
        [_] => "Загрузка видео".to_string(),
        many => format!("Загрузка видео ({})", many.len()),
    };
    let opt = ImportOptions { kind: b.kind, category: b.category, recursive: false, image_duration: None, folder: b.folder };
    let req = job::Request { entries, quality: b.quality };
    let job = s.engine.jobs.spawn("download_media", &title, Some(&id), move |ctl| job::run(h, tools, req, opt, ctl));
    Ok(Json(json!({ "job": job })))
}

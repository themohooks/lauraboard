//! Media library endpoints.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;

use lb_core::Dur;
use lb_core::config::Role;
use lb_core::media::{MediaItem, MediaKind};
use lb_engine::library::{self, ImportOptions};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

pub async fn list(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<Vec<MediaItem>>> {
    c.require(Role::Viewer)?;
    Ok(Json(channel(&s, &id)?.read(|d| d.library.items.clone())))
}

pub async fn get_one(State(s): State<AppState>, c: Caller, Path((id, media)): Path<(String, String)>) -> ApiResult<Json<MediaItem>> {
    c.require(Role::Viewer)?;
    channel(&s, &id)?.read(|d| d.library.get(&media).cloned()).map(Json).ok_or_else(|| ApiError::not_found(format!("media '{media}'")))
}

/// Add or replace a library item as given (no probing).
pub async fn add(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(item): Json<MediaItem>) -> ApiResult<Json<MediaItem>> {
    c.require(Role::Operator)?;
    let it = item.clone();
    channel(&s, &id)?
        .modify(move |d| {
            d.library.upsert(it);
            Ok(())
        })
        .await?;
    Ok(Json(item))
}

pub async fn update(
    State(s): State<AppState>,
    c: Caller,
    Path((id, media)): Path<(String, String)>,
    Json(mut item): Json<MediaItem>,
) -> ApiResult<Json<MediaItem>> {
    c.require(Role::Operator)?;
    item.id = media;
    add(State(s), c, Path(id), Json(item)).await
}

pub async fn remove(State(s): State<AppState>, c: Caller, Path((id, media)): Path<(String, String)>) -> ApiResult<Json<serde_json::Value>> {
    c.require(Role::Operator)?;
    let used = channel(&s, &id)?
        .read(|d| d.playlists.iter().filter(|p| p.entries.iter().any(|e| e.media == media)).map(|p| p.id.clone()).collect::<Vec<_>>());
    if !used.is_empty() {
        return Err(ApiError::bad_request(format!("media '{media}' is used in playlists: {}", used.join(", "))));
    }
    let m = media.clone();
    channel(&s, &id)?
        .modify(move |d| {
            d.library.remove(&m).ok_or_else(|| lb_core::CoreError::NotFound(format!("media '{m}'")))?;
            Ok(())
        })
        .await?;
    Ok(Json(serde_json::json!({ "ok": true, "removed": media })))
}

#[derive(Deserialize)]
pub struct ImportBody {
    /// Files, directories or URLs (as seen by the server).
    pub paths: Vec<String>,
    #[serde(default)]
    pub kind: Option<MediaKind>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub image_duration: Option<Dur>,
}

/// Import and probe files on the server.
pub async fn import(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(b): Json<ImportBody>,
) -> ApiResult<Json<serde_json::Value>> {
    c.require(Role::Operator)?;
    let h = channel(&s, &id)?;
    let mut data = h.read(|d| d.clone());
    let opt = ImportOptions { kind: b.kind, category: b.category, recursive: b.recursive, image_duration: b.image_duration };
    let (added, errors) = library::import(&mut data, &h.shared.root, &s.engine.app.ffmpeg.ffprobe, &b.paths, &opt).await;
    let lib = data.library;
    h.modify(move |d| {
        d.library = lib;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "added": added, "errors": errors })))
}

/// Re-probe all files: detects deleted or damaged media.
pub async fn check(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    c.require(Role::Operator)?;
    let h = channel(&s, &id)?;
    let mut data = h.read(|d| d.clone());
    let problems = library::reprobe_all(&mut data, &h.shared.root, &s.engine.app.ffmpeg.ffprobe).await;
    let lib = data.library;
    h.modify(move |d| {
        d.library = lib;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "problems": problems })))
}

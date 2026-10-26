//! Media library endpoints.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;

use lb_core::Dur;
use lb_core::config::Role;
use lb_core::media::{MediaItem, MediaKind};
use lb_engine::library::{self, ImportOptions, storage};

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
    let h = channel(&s, &id)?;
    let removed =
        h.modify(move |d| d.library.remove(&m).ok_or_else(|| lb_core::CoreError::NotFound(format!("media '{m}'")).into())).await?;
    // a copy in the channel's storage belongs to the item
    storage::remove_own_file(&h.shared.root, &id, &removed.path);
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
    /// Library folder for the new items.
    #[serde(default)]
    pub folder: String,
    /// Copy files into the channel's own storage (a background job; the
    /// reply is `{"job": id}`) instead of referencing them in place.
    #[serde(default)]
    pub copy: bool,
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
    let opt =
        ImportOptions { kind: b.kind, category: b.category, recursive: b.recursive, image_duration: b.image_duration, folder: b.folder };
    if b.copy {
        let ffprobe = s.engine.app.ffmpeg.ffprobe.clone();
        let n = b.paths.len();
        let title = if n == 1 {
            "Копирование файла в медиатеку".to_string()
        } else {
            format!("Копирование файлов в медиатеку ({n})")
        };
        let job = s.engine.jobs.spawn("import_media", &title, Some(&id), move |ctl| storage::import_copies(h, ffprobe, b.paths, opt, ctl));
        return Ok(Json(serde_json::json!({ "job": job })));
    }
    let mut data = h.read(|d| d.clone());
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

/// All folders of the library (explicit and used by items).
pub async fn folders(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<Vec<String>>> {
    c.require(Role::Viewer)?;
    Ok(Json(channel(&s, &id)?.read(|d| d.library.all_folders())))
}

#[derive(Deserialize)]
pub struct FolderOp {
    /// `create`, `rename`, `delete` or `move` (items into `path`).
    pub op: String,
    #[serde(default)]
    pub path: String,
    /// New path for `rename`.
    #[serde(default)]
    pub to: String,
    /// Media ids for `move`.
    #[serde(default)]
    pub ids: Vec<String>,
}

/// Folder operations; files on disk are not touched.
pub async fn folder_op(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(b): Json<FolderOp>,
) -> ApiResult<Json<Vec<String>>> {
    c.require(Role::Operator)?;
    let h = channel(&s, &id)?;
    h.modify(move |d| {
        let lib = &mut d.library;
        let r = match b.op.as_str() {
            "create" => lib.create_folder(&b.path).map(drop),
            "rename" => lib.rename_folder(&b.path, &b.to).map(drop),
            "delete" => {
                lib.delete_folder(&b.path);
                Ok(())
            }
            "move" => {
                lib.move_to_folder(&b.ids, &b.path);
                Ok(())
            }
            other => Err(format!("unknown folder operation '{other}'")),
        };
        r.map_err(|e| lb_core::CoreError::Invalid(e).into())
    })
    .await?;
    Ok(Json(h.read(|d| d.library.all_folders())))
}

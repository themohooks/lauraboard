//! Channel export / import as ZIP archives (paths on the server; the work
//! runs as a background job, see [`super::jobs`]).

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use lb_core::config::Role;
use lb_engine::transfer::{self, Manifest, Parts};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

#[derive(Deserialize)]
pub struct ExportBody {
    /// Archive to write (on the server).
    pub path: String,
    #[serde(default)]
    pub parts: Parts,
}

/// Start exporting a channel (admin: the archive may hold stream keys).
pub async fn export(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(b): Json<ExportBody>) -> ApiResult<Json<Value>> {
    c.require(Role::Admin)?;
    let name = channel(&s, &id)?.read(|d| d.config.name.clone());
    let path = PathBuf::from(b.path.trim());
    if path.as_os_str().is_empty() {
        return Err(ApiError::bad_request("archive path is empty"));
    }
    let engine = s.engine.clone();
    let title = format!("Экспорт канала «{}»", if name.is_empty() { &id } else { &name });
    let ch = id.clone();
    let job = s.engine.jobs.spawn("export_channel", &title, Some(&id), move |ctl| engine.export_channel(ch, path, b.parts, ctl));
    Ok(Json(json!({ "job": job })))
}

#[derive(Deserialize)]
pub struct ArchivePath {
    pub path: String,
}

/// What an archive contains (to fill the import dialog).
pub async fn inspect(c: Caller, Json(b): Json<ArchivePath>) -> ApiResult<Json<Manifest>> {
    c.require(Role::Admin)?;
    let path = PathBuf::from(b.path.trim());
    tokio::task::spawn_blocking(move || transfer::inspect(&path))
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?
        .map(Json)
        .map_err(ApiError::bad_request)
}

#[derive(Deserialize)]
pub struct ImportBody {
    pub path: String,
    /// New channel id (default: the archive's).
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// Start importing a channel from an archive (admin).
pub async fn import(State(s): State<AppState>, c: Caller, Json(b): Json<ImportBody>) -> ApiResult<Json<Value>> {
    c.require(Role::Admin)?;
    let path = PathBuf::from(b.path.trim());
    let manifest = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || transfer::inspect(&p)).await.map_err(|e| ApiError::bad_request(e.to_string()))?
    }
    .map_err(ApiError::bad_request)?;
    let target = b.id.clone().filter(|i| !i.trim().is_empty()).unwrap_or_else(|| manifest.channel.clone());
    if s.engine.channel(&target).is_ok() {
        return Err(ApiError::bad_request(format!("channel '{target}' already exists")));
    }
    let engine = s.engine.clone();
    let title = format!("Импорт канала «{}»", if manifest.name.is_empty() { &manifest.channel } else { &manifest.name });
    let job = s.engine.jobs.spawn("import_channel", &title, Some(&target), move |ctl| engine.import_channel(path, b.id, b.name, ctl));
    Ok(Json(json!({ "job": job, "channel": target })))
}

//! Ready-made backgrounds of a channel: generating them (a background
//! job: library pictures, video loops, plate textures) and the plate
//! textures for the graphics editor.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use lb_core::config::Role;
use lb_engine::library::backgrounds::{self, Parts};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
pub struct GenerateBody {
    #[serde(default = "yes")]
    pub stills: bool,
    #[serde(default = "yes")]
    pub loops: bool,
    #[serde(default = "yes")]
    pub plates: bool,
}

/// Start the job for channel `id`; returns the job id.
pub fn start(s: &AppState, id: &str, parts: Parts) -> ApiResult<u64> {
    let h = channel(s, id)?;
    Ok(backgrounds::spawn(&s.engine, h, parts))
}

/// `POST /channels/{id}/media/backgrounds` `{"stills":true,"loops":true,"plates":true}`.
pub async fn generate(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(b): Json<GenerateBody>) -> ApiResult<Json<Value>> {
    c.require(Role::Operator)?;
    let job = start(&s, &id, Parts { stills: b.stills, loops: b.loops, plates: b.plates })?;
    Ok(Json(json!({ "job": job })))
}

/// `GET /channels/{id}/graphics/backgrounds`: plate textures (made on the
/// first request) as `[{path, title, file}]`.
pub async fn plates(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let dir = h.read(|d| d.dir.clone());
    let root = h.shared.root.clone();
    let list = tokio::task::spawn_blocking(move || backgrounds::ensure_plates(&dir, &root))
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?
        .map_err(ApiError::bad_request)?;
    let v: Vec<Value> = list
        .into_iter()
        .map(|(path, title)| {
            let file = path.rsplit('/').next().unwrap_or_default().to_string();
            json!({ "path": path, "title": title, "file": file })
        })
        .collect();
    Ok(Json(json!(v)))
}

/// `GET /channels/{id}/graphics/backgrounds/{file}`: one texture (PNG).
pub async fn plate_file(State(s): State<AppState>, c: Caller, Path((id, file)): Path<(String, String)>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    if file.contains(['/', '\\']) || file.contains("..") || !file.ends_with(".png") {
        return Err(ApiError::bad_request("bad file name"));
    }
    let h = channel(&s, &id)?;
    let path = h.read(|d| d.dir.join(backgrounds::PLATES_DIR).join(&file));
    let bytes = tokio::fs::read(&path).await.map_err(|_| ApiError::not_found(format!("texture '{file}'")))?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "max-age=3600")], bytes).into_response())
}

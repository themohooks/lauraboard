//! Channel avatar: `GET /channels/{id}/avatar.png`, `PUT` the picture
//! (raw bytes of a PNG / JPEG / WebP, cut to a 256×256 square), `DELETE`.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use lb_core::config::Role;
use lb_engine::channel::avatar;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

pub async fn get(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let path = avatar::path(&h.read(|d| d.dir.clone()));
    let bytes = tokio::fs::read(&path).await.map_err(|_| ApiError::not_found(format!("avatar of '{id}'")))?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-cache")], bytes).into_response())
}

pub async fn put(State(s): State<AppState>, c: Caller, Path(id): Path<String>, body: Bytes) -> ApiResult<Json<Value>> {
    c.require(Role::Admin)?;
    let h = channel(&s, &id)?;
    let dir = h.read(|d| d.dir.clone());
    tokio::task::spawn_blocking(move || avatar::save(&dir, &body)).await.map_err(|e| ApiError::bad_request(e.to_string()))?.map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "avatar": h.status().avatar })))
}

pub async fn delete(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    c.require(Role::Admin)?;
    let h = channel(&s, &id)?;
    avatar::remove(&h.read(|d| d.dir.clone())).map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "ok": true })))
}

//! Background jobs: progress of media copying and channel export / import.

use axum::Json;
use axum::extract::{Path, State};

use lb_core::config::Role;
use lb_engine::jobs::JobInfo;

use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

pub async fn list(State(s): State<AppState>, c: Caller) -> ApiResult<Json<Vec<JobInfo>>> {
    c.require(Role::Viewer)?;
    Ok(Json(s.engine.jobs.list()))
}

pub async fn get_one(State(s): State<AppState>, c: Caller, Path(id): Path<u64>) -> ApiResult<Json<JobInfo>> {
    c.require(Role::Viewer)?;
    s.engine.jobs.get(id).map(Json).ok_or_else(|| ApiError::not_found(format!("job {id}")))
}

/// Cancel a running job or remove a finished one from the list.
pub async fn cancel(State(s): State<AppState>, c: Caller, Path(id): Path<u64>) -> ApiResult<Json<serde_json::Value>> {
    c.require(Role::Operator)?;
    if !s.engine.jobs.cancel_or_dismiss(id) {
        return Err(ApiError::not_found(format!("job {id}")));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

//! Playback and graphics control.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;

use lb_core::config::Role;
use lb_engine::Command;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

type Reply = Json<serde_json::Value>;

fn ok(msg: String) -> Reply {
    Json(serde_json::json!({ "ok": true, "message": msg }))
}

async fn run(s: &AppState, c: &Caller, id: &str, cmd: Command) -> ApiResult<Reply> {
    c.require(Role::Operator)?;
    tracing::info!(user = %c.name, channel = id, "command {:?}", cmd);
    Ok(ok(channel(s, id)?.send(cmd).await?))
}

/// `POST /channels/{id}/command` with a JSON command, e.g.
/// `{"cmd":"play_now","media":"intro"}`.
pub async fn command(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(cmd): Json<Command>) -> ApiResult<Reply> {
    if cmd == Command::Shutdown {
        return Err(ApiError::bad_request("use DELETE /channels/{id} to remove a channel"));
    }
    run(&s, &c, &id, cmd).await
}

pub async fn start(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Reply> {
    run(&s, &c, &id, Command::Start).await
}

pub async fn stop(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Reply> {
    run(&s, &c, &id, Command::Stop).await
}

pub async fn skip(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Reply> {
    run(&s, &c, &id, Command::Skip).await
}

#[derive(Deserialize)]
pub struct VarBody {
    pub value: Option<String>,
}

/// `PUT /channels/{id}/var/{key}` `{"value": "Breaking news…"}`.
pub async fn set_var(
    State(s): State<AppState>,
    c: Caller,
    Path((id, key)): Path<(String, String)>,
    Json(b): Json<VarBody>,
) -> ApiResult<Reply> {
    run(&s, &c, &id, Command::SetVar { key, value: b.value }).await
}

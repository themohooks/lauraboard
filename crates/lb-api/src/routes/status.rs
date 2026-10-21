//! Read-only status endpoints.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use lb_core::config::Role;
use lb_core::planner::{self, PlanInput};
use lb_core::rundown::Rundown;
use lb_engine::ChannelStatus;
use lb_engine::persist::{asrun, state as rstate};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

pub async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "name": "laurabrod", "version": env!("CARGO_PKG_VERSION"), "api": 1 }))
}

pub async fn channels(State(s): State<AppState>, c: Caller) -> ApiResult<Json<Vec<ChannelStatus>>> {
    c.require(Role::Viewer)?;
    Ok(Json(s.engine.channels().map(|h| h.status()).collect()))
}

pub async fn status(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<ChannelStatus>> {
    c.require(Role::Viewer)?;
    Ok(Json(channel(&s, &id)?.status()))
}

#[derive(Deserialize)]
pub struct RundownQuery {
    /// Limit number of items.
    pub limit: Option<usize>,
}

pub async fn rundown(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Query(q): Query<RundownQuery>,
) -> ApiResult<Json<Rundown>> {
    c.require(Role::Viewer)?;
    let mut r = (*channel(&s, &id)?.rundown()).clone();
    if let Some(l) = q.limit {
        r.items.truncate(l);
    }
    Ok(Json(r))
}

pub async fn preview(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let frame = channel(&s, &id)?.preview().ok_or_else(|| ApiError::not_found("no preview frame yet"))?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], frame.to_vec()).into_response())
}

#[derive(Deserialize)]
pub struct PlanQuery {
    /// Start (default: now), `YYYY-MM-DDTHH:MM:SS`.
    pub from: Option<lb_core::WallTime>,
    /// Hours ahead (default 24, max 24*14).
    pub hours: Option<i64>,
}

/// Plan preview for an arbitrary window (independent of the live rundown).
pub async fn plan_preview(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Query(q): Query<PlanQuery>,
) -> ApiResult<Json<Rundown>> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let from = q.from.unwrap_or_else(|| chrono::Local::now().naive_local());
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 14);
    let r = h.read(|d| {
        let saved = rstate::load(&d.state_dir());
        let hist = asrun::load_history(&d.asrun_dir(), from.date(), (d.config.planning.history.ms() / 86_400_000).max(1));
        let input = PlanInput {
            library: &d.library,
            playlists: &d.playlists,
            schedules: &d.schedules,
            rules: &d.rules,
            history: &hist,
            mode: &d.config.planning.mode,
            seed: d.config.planning.seed,
            force_schedule: saved.force_schedule.as_deref(),
        };
        planner::plan(&input, &saved.plan, from, lb_core::add_ms(from, hours * 3_600_000))
    });
    Ok(Json(r))
}

pub async fn validate(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<lb_core::validate::Report>> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let root = h.shared.root.clone();
    Ok(Json(h.read(|d| lb_core::validate::validate_channel_full(d, &root))))
}

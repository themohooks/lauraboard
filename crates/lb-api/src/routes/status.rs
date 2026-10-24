//! Read-only status endpoints.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use lb_core::config::Role;
use lb_core::planner::{self, PlanInput};
use lb_core::rules::RuleSet;
use lb_core::rundown::Rundown;
use lb_core::schedule::ScheduleSet;
use lb_engine::{ChannelHandle, ChannelStatus};
use lb_engine::persist::{asrun, state as rstate};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

pub async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "name": "laurabrod", "version": env!("CARGO_PKG_VERSION"), "api": 1 }))
}

/// Host metrics: CPU, memory, disks, network, Laurabrod and FFmpeg processes.
pub async fn system(c: Caller) -> ApiResult<Json<lb_engine::system::SystemInfo>> {
    c.require(Role::Viewer)?;
    let info = tokio::task::spawn_blocking(lb_engine::system::snapshot)
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(info))
}

pub async fn channels(State(s): State<AppState>, c: Caller) -> ApiResult<Json<Vec<ChannelStatus>>> {
    c.require(Role::Viewer)?;
    Ok(Json(s.engine.channels().iter().map(|h| h.status()).collect()))
}

#[derive(Deserialize)]
pub struct NewChannel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Copy settings (video, encoder, …; not outputs) from this channel.
    #[serde(default)]
    pub copy_from: Option<String>,
}

/// Create a channel while running (admin).
pub async fn create_channel(State(s): State<AppState>, c: Caller, Json(v): Json<NewChannel>) -> ApiResult<Json<ChannelStatus>> {
    c.require(Role::Admin)?;
    let h = s.engine.create_channel(v.id.trim(), &v.name, v.copy_from.as_deref().filter(|x| !x.is_empty()))?;
    // every channel comes with ready-made backgrounds (copied in the background)
    lb_engine::library::backgrounds::spawn(&s.engine, h.clone());
    Ok(Json(h.status()))
}

#[derive(Deserialize)]
pub struct ChannelMeta {
    /// New display name (empty = unchanged).
    #[serde(default)]
    pub name: String,
    /// New id / folder name (empty = unchanged; channel must be stopped).
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub autostart: Option<bool>,
}

/// Rename a channel, change its id or autostart (admin).
pub async fn update_channel(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(v): Json<ChannelMeta>,
) -> ApiResult<Json<ChannelStatus>> {
    c.require(Role::Admin)?;
    let h = s.engine.rename_channel(&id, &v.id, &v.name).await?;
    if let Some(a) = v.autostart {
        h.modify(move |d| {
            d.config.autostart = a;
            Ok(())
        })
        .await?;
    }
    Ok(Json(h.status()))
}

#[derive(Deserialize)]
pub struct Duplicate {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// Copy a channel with all its content (admin).
pub async fn duplicate_channel(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(v): Json<Duplicate>,
) -> ApiResult<Json<ChannelStatus>> {
    c.require(Role::Admin)?;
    let h = s.engine.duplicate_channel(&id, v.id.trim(), &v.name).await?;
    Ok(Json(h.status()))
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    /// Erase the folder instead of moving it to `deleted/`.
    #[serde(default)]
    pub purge: bool,
}

/// Remove a channel (admin).
pub async fn delete_channel(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    c.require(Role::Admin)?;
    let moved = s.engine.delete_channel(&id, q.purge).await?;
    Ok(Json(serde_json::json!({ "ok": true, "moved_to": moved.map(|p| p.to_string_lossy().into_owned()) })))
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
    Ok(Json(plan_with(&h, q.from, q.hours, PlanDraft::default()).await))
}

/// Unsaved documents substituted for the stored ones when planning.
#[derive(Deserialize, Default)]
pub struct PlanDraft {
    #[serde(default)]
    pub rules: Option<RuleSet>,
    #[serde(default)]
    pub schedules: Option<ScheduleSet>,
}

#[derive(Deserialize)]
pub struct PlanBody {
    #[serde(default)]
    pub from: Option<lb_core::WallTime>,
    #[serde(default)]
    pub hours: Option<i64>,
    #[serde(flatten)]
    pub draft: PlanDraft,
}

/// Plan preview with draft rules and/or schedules (nothing is stored), so
/// editors can check their changes before saving.
pub async fn plan_draft(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(b): Json<PlanBody>,
) -> ApiResult<Json<Rundown>> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    Ok(Json(plan_with(&h, b.from, b.hours, b.draft).await))
}

/// Plan `hours` from `from` (default: now, 24 h, max 14 days) on a blocking
/// thread so long windows do not stall the API.
async fn plan_with(h: &ChannelHandle, from: Option<lb_core::WallTime>, hours: Option<i64>, draft: PlanDraft) -> Rundown {
    let from = from.unwrap_or_else(|| chrono::Local::now().naive_local());
    let hours = hours.unwrap_or(24).clamp(1, 24 * 14);
    let h = h.clone();
    let job = move || {
        h.read(|d| {
            let saved = rstate::load(&d.state_dir());
            let hist = asrun::load_history(&d.asrun_dir(), from.date(), (d.config.planning.history.ms() / 86_400_000).max(1));
            let input = PlanInput {
                library: &d.library,
                playlists: &d.playlists,
                schedules: draft.schedules.as_ref().unwrap_or(&d.schedules),
                rules: draft.rules.as_ref().unwrap_or(&d.rules),
                history: &hist,
                mode: &d.config.planning.mode,
                seed: d.config.planning.seed,
                force_schedule: saved.force_schedule.as_deref(),
            };
            planner::plan(&input, &saved.plan, from, lb_core::add_ms(from, hours * 3_600_000))
        })
    };
    tokio::task::spawn_blocking(job).await.unwrap_or_default()
}

pub async fn validate(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<lb_core::validate::Report>> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let root = h.shared.root.clone();
    Ok(Json(h.read(|d| lb_core::validate::validate_channel_full(d, &root))))
}

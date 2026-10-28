//! Configuration documents: schedules, rules, graphics, channel settings.
//! Each document is replaced as a whole (GET → edit → PUT). Files the
//! graphics and the settings use are copied into the channel folder first.

use axum::Json;
use axum::extract::{Path, State};

use lb_core::config::{ChannelConfig, Role};
use lb_core::graphics::GraphicsConfig;
use lb_core::project::ChannelData;
use lb_core::rules::RuleSet;
use lb_core::schedule::ScheduleSet;
use lb_core::validate::{Report, validate_channel};
use lb_engine::channel::assets;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

/// Apply `f` to a copy, validate, then store (rejecting new errors).
async fn update(s: &AppState, id: &str, f: impl FnOnce(&mut ChannelData) + Send + 'static) -> ApiResult<Json<Report>> {
    let h = channel(s, id)?;
    let mut copy = h.read(|d| d.clone());
    // errors that were already there (in another document) must not block
    // an unrelated save; only reject what this change introduces
    let before = validate_channel(&copy);
    f(&mut copy);
    let report = validate_channel(&copy);
    let msg = report
        .issues
        .iter()
        .filter(|i| i.severity == lb_core::validate::Severity::Error && !before.issues.contains(i))
        .map(|i| format!("{}: {}", i.location, i.message))
        .collect::<Vec<_>>();
    if !msg.is_empty() {
        return Err(ApiError::bad_request(format!("validation failed: {}", msg.join("; "))));
    }
    h.modify(move |d| {
        *d = copy;
        Ok(())
    })
    .await?;
    Ok(Json(report))
}

pub async fn get_schedules(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<ScheduleSet>> {
    c.require(Role::Viewer)?;
    Ok(Json(channel(&s, &id)?.read(|d| d.schedules.clone())))
}

pub async fn put_schedules(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(v): Json<ScheduleSet>,
) -> ApiResult<Json<Report>> {
    c.require(Role::Operator)?;
    update(&s, &id, move |d| d.schedules = v).await
}

pub async fn get_rules(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<RuleSet>> {
    c.require(Role::Viewer)?;
    Ok(Json(channel(&s, &id)?.read(|d| d.rules.clone())))
}

pub async fn put_rules(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(v): Json<RuleSet>) -> ApiResult<Json<Report>> {
    c.require(Role::Operator)?;
    update(&s, &id, move |d| d.rules = v).await
}

pub async fn get_graphics(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<GraphicsConfig>> {
    c.require(Role::Viewer)?;
    Ok(Json(channel(&s, &id)?.read(|d| d.graphics.clone())))
}

pub async fn put_graphics(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(mut v): Json<GraphicsConfig>,
) -> ApiResult<Json<Report>> {
    c.require(Role::Operator)?;
    let (root, dir) = (s.engine.project_root.clone(), channel(&s, &id)?.read(|d| d.dir.clone()));
    let v = tokio::task::spawn_blocking(move || {
        assets::adopt_graphics(&root, &dir, &mut v);
        v
    })
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    update(&s, &id, move |d| d.graphics = v).await
}

/// Channel settings may contain stream keys: admin only.
pub async fn get_config(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<ChannelConfig>> {
    c.require(Role::Admin)?;
    Ok(Json(channel(&s, &id)?.read(|d| d.config.clone())))
}

pub async fn put_config(
    State(s): State<AppState>,
    c: Caller,
    Path(id): Path<String>,
    Json(mut v): Json<ChannelConfig>,
) -> ApiResult<Json<Report>> {
    c.require(Role::Admin)?;
    v.id = id.clone();
    if v.local_hls.enabled {
        if v.local_hls.port < 1024 {
            return Err(ApiError::bad_request("HLS port must be 1024–65535"));
        }
        for other in s.engine.channels() {
            if other.id() != id && other.read(|d| d.config.local_hls.enabled && d.config.local_hls.port == v.local_hls.port) {
                return Err(ApiError::bad_request("HLS port is already used by another channel"));
            }
        }
        let previous = channel(&s, &id)?.read(|d| d.config.local_hls.clone());
        if !previous.enabled || previous.port != v.local_hls.port {
            std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, v.local_hls.port))
                .map_err(|e| ApiError::bad_request(format!("HLS port: {e}")))?;
        }
    }

    let (root, dir) = (s.engine.project_root.clone(), channel(&s, &id)?.read(|d| d.dir.clone()));
    let v = tokio::task::spawn_blocking(move || {
        assets::adopt_config(&root, &dir, &mut v);
        v
    })
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    update(&s, &id, move |d| d.config = v).await
}

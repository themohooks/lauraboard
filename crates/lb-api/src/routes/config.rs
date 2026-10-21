//! Configuration documents: schedules, rules, graphics, channel settings.
//! Each document is replaced as a whole (GET → edit → PUT).

use axum::Json;
use axum::extract::{Path, State};

use lb_core::config::{ChannelConfig, Role};
use lb_core::graphics::GraphicsConfig;
use lb_core::project::ChannelData;
use lb_core::rules::RuleSet;
use lb_core::schedule::ScheduleSet;
use lb_core::validate::{Report, validate_channel};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

/// Apply `f` to a copy, validate, then store (rejecting new errors).
async fn update(s: &AppState, id: &str, f: impl FnOnce(&mut ChannelData) + Send + 'static) -> ApiResult<Json<Report>> {
    let h = channel(s, id)?;
    let mut copy = h.read(|d| d.clone());
    f(&mut copy);
    let report = validate_channel(&copy);
    if report.has_errors() {
        let msg = report
            .issues
            .iter()
            .filter(|i| i.severity == lb_core::validate::Severity::Error)
            .map(|i| format!("{}: {}", i.location, i.message))
            .collect::<Vec<_>>();
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
    Json(v): Json<GraphicsConfig>,
) -> ApiResult<Json<Report>> {
    c.require(Role::Operator)?;
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
    update(&s, &id, move |d| d.config = v).await
}

//! As-run log, statistics and application log.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use serde::Deserialize;

use lb_core::config::Role;
use lb_engine::persist::asrun;
use lb_engine::stats;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::ApiResult;

#[derive(Deserialize)]
pub struct Range {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// `json` (default) or `csv`.
    pub format: Option<String>,
}

fn range(r: &Range) -> (NaiveDate, NaiveDate) {
    let today = chrono::Local::now().date_naive();
    let to = r.to.unwrap_or(today);
    (r.from.unwrap_or(to), to)
}

fn csv(body: String, name: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        body,
    )
        .into_response()
}

pub async fn asrun(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Query(q): Query<Range>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let (from, to) = range(&q);
    let dir = channel(&s, &id)?.read(|d| d.asrun_dir());
    let recs = asrun::read_range(&dir, from, to);
    Ok(match q.format.as_deref() {
        Some("csv") => csv(stats::asrun_csv(&recs), &format!("asrun-{id}-{from}-{to}.csv")),
        _ => Json(recs).into_response(),
    })
}

pub async fn stats(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Query(q): Query<Range>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let (from, to) = range(&q);
    let dir = channel(&s, &id)?.read(|d| d.asrun_dir());
    let r = stats::report(&dir, from, to);
    Ok(match q.format.as_deref() {
        Some("csv") => csv(stats::stats_csv(&r.by_media), &format!("stats-{id}-{from}-{to}.csv")),
        _ => Json(r).into_response(),
    })
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub lines: Option<usize>,
}

/// Tail of today's application log file.
pub async fn logs(State(s): State<AppState>, c: Caller, Query(q): Query<LogQuery>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let dir = s.engine.project_root.join(&s.engine.app.log.dir);
    let n = q.lines.unwrap_or(200).min(5000);
    let latest = std::fs::read_dir(&dir)
        .ok()
        .and_then(|rd| rd.flatten().filter(|e| e.path().is_file()).max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok()))
        .map(|e| e.path());
    let text = latest.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(n)..].join("\n");
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], tail).into_response())
}

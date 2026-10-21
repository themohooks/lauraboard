//! Playlist endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;

use lb_core::CoreError;
use lb_core::config::Role;
use lb_core::playlist::{Playlist, PlaylistEntry};
use lb_core::project::m3u;

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

#[derive(serde::Serialize)]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub entries: usize,
    pub duration_ms: i64,
    #[serde(rename = "loop")]
    pub looped: bool,
    pub shuffle: bool,
}

pub async fn list(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<Vec<PlaylistSummary>>> {
    c.require(Role::Viewer)?;
    Ok(Json(channel(&s, &id)?.read(|d| {
        d.playlists
            .iter()
            .map(|p| PlaylistSummary {
                id: p.id.clone(),
                name: p.name.clone(),
                entries: p.entries.len(),
                duration_ms: p.total_duration(&d.library),
                looped: p.looped,
                shuffle: p.shuffle,
            })
            .collect()
    })))
}

pub async fn get_one(State(s): State<AppState>, c: Caller, Path((id, pl)): Path<(String, String)>) -> ApiResult<Json<Playlist>> {
    c.require(Role::Viewer)?;
    channel(&s, &id)?.read(|d| d.playlist(&pl).cloned()).map(Json).ok_or_else(|| ApiError::not_found(format!("playlist '{pl}'")))
}

fn check_media(d: &lb_core::project::ChannelData, p: &Playlist) -> lb_core::Result<()> {
    match p.entries.iter().find(|e| !d.library.contains(&e.media)) {
        Some(e) => Err(CoreError::Invalid(format!("unknown media '{}'", e.media))),
        None => Ok(()),
    }
}

pub async fn create(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(p): Json<Playlist>) -> ApiResult<Json<Playlist>> {
    c.require(Role::Operator)?;
    let out = p.clone();
    channel(&s, &id)?
        .modify(move |d| {
            if d.playlist(&p.id).is_some() {
                return Err(CoreError::Invalid(format!("playlist '{}' exists", p.id)).into());
            }
            check_media(d, &p)?;
            d.playlists.push(p);
            Ok(())
        })
        .await?;
    Ok(Json(out))
}

pub async fn replace(
    State(s): State<AppState>,
    c: Caller,
    Path((id, pl)): Path<(String, String)>,
    Json(mut p): Json<Playlist>,
) -> ApiResult<Json<Playlist>> {
    c.require(Role::Operator)?;
    p.id = pl.clone();
    let out = p.clone();
    channel(&s, &id)?
        .modify(move |d| {
            check_media(d, &p)?;
            match d.playlist_mut(&pl) {
                Some(x) => *x = p,
                None => d.playlists.push(p),
            }
            Ok(())
        })
        .await?;
    Ok(Json(out))
}

pub async fn remove(State(s): State<AppState>, c: Caller, Path((id, pl)): Path<(String, String)>) -> ApiResult<Json<serde_json::Value>> {
    c.require(Role::Operator)?;
    channel(&s, &id)?
        .modify(move |d| {
            let before = d.playlists.len();
            d.playlists.retain(|p| p.id != pl);
            if d.playlists.len() == before {
                return Err(CoreError::NotFound(format!("playlist '{pl}'")).into());
            }
            Ok(())
        })
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AddEntry {
    pub media: String,
    /// Insert position (default: end).
    pub index: Option<usize>,
    #[serde(default)]
    pub pinned: bool,
}

pub async fn add_entry(
    State(s): State<AppState>,
    c: Caller,
    Path((id, pl)): Path<(String, String)>,
    Json(b): Json<AddEntry>,
) -> ApiResult<Json<Playlist>> {
    c.require(Role::Operator)?;
    let p = channel(&s, &id)?
        .modify(move |d| {
            if !d.library.contains(&b.media) {
                return Err(CoreError::Invalid(format!("unknown media '{}'", b.media)).into());
            }
            let p = d.playlist_mut(&pl).ok_or_else(|| CoreError::NotFound(format!("playlist '{pl}'")))?;
            let mut e = PlaylistEntry::new(b.media);
            e.pinned = b.pinned;
            let idx = b.index.unwrap_or(p.entries.len()).min(p.entries.len());
            p.entries.insert(idx, e);
            Ok(p.clone())
        })
        .await?;
    Ok(Json(p))
}

pub async fn remove_entry(
    State(s): State<AppState>,
    c: Caller,
    Path((id, pl, idx)): Path<(String, String, usize)>,
) -> ApiResult<Json<Playlist>> {
    c.require(Role::Operator)?;
    let p = channel(&s, &id)?
        .modify(move |d| {
            let p = d.playlist_mut(&pl).ok_or_else(|| CoreError::NotFound(format!("playlist '{pl}'")))?;
            if idx >= p.entries.len() {
                return Err(CoreError::Invalid(format!("index {idx} out of range")).into());
            }
            p.entries.remove(idx);
            Ok(p.clone())
        })
        .await?;
    Ok(Json(p))
}

#[derive(Deserialize)]
pub struct MoveBody {
    pub from: usize,
    pub to: usize,
}

pub async fn move_entry(
    State(s): State<AppState>,
    c: Caller,
    Path((id, pl)): Path<(String, String)>,
    Json(b): Json<MoveBody>,
) -> ApiResult<Json<Playlist>> {
    c.require(Role::Operator)?;
    let p = channel(&s, &id)?
        .modify(move |d| {
            let p = d.playlist_mut(&pl).ok_or_else(|| CoreError::NotFound(format!("playlist '{pl}'")))?;
            if !p.move_entry(b.from, b.to) {
                return Err(CoreError::Invalid("index out of range".into()).into());
            }
            Ok(p.clone())
        })
        .await?;
    Ok(Json(p))
}

pub async fn export_m3u(State(s): State<AppState>, c: Caller, Path((id, pl)): Path<(String, String)>) -> ApiResult<impl IntoResponse> {
    c.require(Role::Viewer)?;
    let text = channel(&s, &id)?
        .read(|d| d.playlist(&pl).map(|p| m3u::export(p, &d.library)))
        .ok_or_else(|| ApiError::not_found(format!("playlist '{pl}'")))?;
    Ok(([(header::CONTENT_TYPE, "audio/x-mpegurl")], text))
}

/// Body: M3U text. Entries are matched to library items by path.
pub async fn import_m3u(
    State(s): State<AppState>,
    c: Caller,
    Path((id, pl)): Path<(String, String)>,
    body: String,
) -> ApiResult<Json<serde_json::Value>> {
    c.require(Role::Operator)?;
    let entries = m3u::parse(&body);
    let (missing, count) = channel(&s, &id)?
        .modify(move |d| {
            let (p, missing) = m3u::to_playlist(&pl, &entries, &d.library);
            let n = p.entries.len();
            match d.playlist_mut(&pl) {
                Some(x) => x.entries = p.entries,
                None => d.playlists.push(p),
            }
            Ok((missing, n))
        })
        .await?;
    Ok(Json(serde_json::json!({ "entries": count, "not_in_library": missing })))
}

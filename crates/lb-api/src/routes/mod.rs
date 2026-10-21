//! REST routes under `/api/v1`.
//!
//! | area | module |
//! |------|--------|
//! | channels, status, rundown, preview | [`status`] |
//! | playback / graphics commands | [`control`] |
//! | media library | [`library`] |
//! | playlists | [`playlists`] |
//! | schedules, rules, graphics, channel config | [`config`] |
//! | as-run, statistics, logs | [`reports`] |

pub mod config;
pub mod control;
pub mod library;
pub mod playlists;
pub mod reports;
pub mod status;

use axum::Router;
use axum::routing::{get, post, put};
use tower_http::cors::CorsLayer;

use crate::AppState;
use crate::error::ApiResult;
use lb_engine::ChannelHandle;

/// Resolve a channel id from the path.
pub(crate) fn channel(state: &AppState, id: &str) -> ApiResult<ChannelHandle> {
    Ok(state.engine.channel(id)?.clone())
}

pub fn router(state: AppState) -> Router {
    let ch = Router::new()
        .route("/status", get(status::status))
        .route("/rundown", get(status::rundown))
        .route("/preview.jpg", get(status::preview))
        .route("/plan", get(status::plan_preview))
        .route("/validate", get(status::validate))
        .route("/command", post(control::command))
        .route("/start", post(control::start))
        .route("/stop", post(control::stop))
        .route("/skip", post(control::skip))
        .route("/media", get(library::list).post(library::add))
        .route("/media/import", post(library::import))
        .route("/media/check", post(library::check))
        .route("/media/{media}", get(library::get_one).put(library::update).delete(library::remove))
        .route("/playlists", get(playlists::list).post(playlists::create))
        .route("/playlists/{pl}", get(playlists::get_one).put(playlists::replace).delete(playlists::remove))
        .route("/playlists/{pl}/entries", post(playlists::add_entry))
        .route("/playlists/{pl}/entries/{idx}", axum::routing::delete(playlists::remove_entry))
        .route("/playlists/{pl}/move", post(playlists::move_entry))
        .route("/playlists/{pl}/m3u", get(playlists::export_m3u).post(playlists::import_m3u))
        .route("/schedules", get(config::get_schedules).put(config::put_schedules))
        .route("/rules", get(config::get_rules).put(config::put_rules))
        .route("/graphics", get(config::get_graphics).put(config::put_graphics))
        .route("/config", get(config::get_config).put(config::put_config))
        .route("/asrun", get(reports::asrun))
        .route("/stats", get(reports::stats));
    let api = Router::new()
        .route("/channels", get(status::channels))
        .route("/events", get(crate::ws::events_all))
        .route("/logs", get(reports::logs))
        .route("/version", get(status::version))
        .nest("/channels/{id}", ch.route("/events", get(crate::ws::events_channel)))
        .route("/channels/{id}/var/{key}", put(control::set_var));
    Router::new().nest("/api/v1", api).layer(CorsLayer::permissive()).with_state(state)
}

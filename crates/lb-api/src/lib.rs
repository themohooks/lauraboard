//! Local HTTP API (REST + WebSocket) for controlling channels.
//!
//! All routes live under `/api/v1`. See `docs/api.md` for the reference.
//!
//! * [`auth`] — bearer tokens and roles
//! * [`error`] — JSON error responses
//! * [`routes`] — REST endpoints grouped by area
//! * [`ws`] — real-time event stream

pub mod auth;
pub mod error;
pub mod routes;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use lb_core::config::ApiConfig;
use lb_engine::Engine;

/// Shared state for handlers.
#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub api: Arc<ApiConfig>,
}

/// Build the router (useful for tests).
pub fn router(state: AppState) -> axum::Router {
    routes::router(state)
}

/// Serve the API until `shutdown` resolves.
pub async fn serve(engine: Arc<Engine>, shutdown: impl std::future::Future<Output = ()> + Send + 'static) -> std::io::Result<()> {
    let api = engine.app.api.clone();
    let addr: SocketAddr = api.bind.parse().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("api.bind: {e}")))?;
    if !addr.ip().is_loopback() && api.tokens.is_empty() {
        tracing::warn!("API bound to {addr} without tokens: only loopback clients are allowed");
    }
    let state = AppState { engine, api: Arc::new(api) };
    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("API listening on http://{addr}/api/v1");
    axum::serve(listener, app).with_graceful_shutdown(shutdown).await
}

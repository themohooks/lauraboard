//! Authentication: `Authorization: Bearer <token>` (or `?token=` for
//! WebSocket clients). Without configured tokens, loopback clients get admin
//! access and everyone else is rejected.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

use lb_core::config::Role;

use crate::AppState;
use crate::error::ApiError;

/// Authenticated caller.
#[derive(Debug, Clone)]
pub struct Caller {
    pub name: String,
    pub role: Role,
}

impl Caller {
    pub fn require(&self, role: Role) -> Result<(), ApiError> {
        if self.role >= role { Ok(()) } else { Err(ApiError::forbidden(format!("requires {role:?} role"))) }
    }
}

fn token_from(parts: &Parts) -> Option<String> {
    if let Some(h) = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok())
        && let Some(t) = h.strip_prefix("Bearer ")
    {
        return Some(t.trim().to_string());
    }
    parts.uri.query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=").map(str::to_string)))
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let tokens = &state.api.tokens;
        if tokens.is_empty() {
            let loopback = parts.extensions.get::<ConnectInfo<SocketAddr>>().is_none_or(|ci| ci.0.ip().is_loopback());
            return if loopback { Ok(Caller { name: "local".into(), role: Role::Admin }) } else { Err(ApiError::unauthorized()) };
        }
        let t = token_from(parts).ok_or_else(ApiError::unauthorized)?;
        tokens
            .iter()
            .find(|k| constant_time_eq(k.token.as_bytes(), t.as_bytes()))
            .map(|k| Caller { name: k.name.clone(), role: k.role })
            .ok_or_else(ApiError::unauthorized)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

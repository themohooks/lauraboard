//! Errors rendered as `{"error": "..."}` with a proper status code.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        ApiError { status, message: message.into() }
    }
    pub fn bad_request(m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, m)
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, m)
    }
    pub fn forbidden(m: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, m)
    }
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "missing or invalid token")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.message }))).into_response()
    }
}

impl From<lb_engine::EngineError> for ApiError {
    fn from(e: lb_engine::EngineError) -> Self {
        use lb_engine::EngineError as E;
        let status = match &e {
            E::Core(c) => return ApiError::from_core(c),
            E::UnknownChannel(_) => StatusCode::NOT_FOUND,
            E::AlreadyRunning(_) => StatusCode::CONFLICT,
            E::Other(_) | E::InvalidConfig(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError::new(status, e.to_string())
    }
}

impl ApiError {
    fn from_core(e: &lb_core::CoreError) -> Self {
        let status = match e {
            lb_core::CoreError::NotFound(_) => StatusCode::NOT_FOUND,
            lb_core::CoreError::Invalid(_) | lb_core::CoreError::Parse { .. } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError::new(status, e.to_string())
    }
}

impl From<lb_core::CoreError> for ApiError {
    fn from(e: lb_core::CoreError) -> Self {
        ApiError::from_core(&e)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

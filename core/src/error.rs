//! Crate-wide error type + axum response mapping.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    BadRequest(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("database error: {0}")]
    Db(#[from] diesel::result::Error),

    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("vault io error: {0}")]
    VaultIo(#[from] std::io::Error),

    #[error("engine offline: {0}")]
    EngineOffline(String),

    #[error("other: {0}")]
    Other(#[from] anyhow::Error),
}

impl CoreError {
    pub fn status(&self) -> StatusCode {
        match self {
            CoreError::NotFound(_) => StatusCode::NOT_FOUND,
            CoreError::BadRequest(_) => StatusCode::BAD_REQUEST,
            CoreError::Conflict(_) => StatusCode::CONFLICT,
            CoreError::EngineOffline(_) => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            CoreError::NotFound(_) => "not_found",
            CoreError::BadRequest(_) => "bad_request",
            CoreError::Conflict(_) => "conflict",
            CoreError::Db(_) => "database_error",
            CoreError::Pool(_) => "pool_error",
            CoreError::VaultIo(_) => "vault_io_error",
            CoreError::EngineOffline(_) => "engine_offline",
            CoreError::Other(_) => "internal_error",
        }
    }
}

impl IntoResponse for CoreError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();
        // Log server-class errors but never leak internals to the client.
        if status.is_server_error() {
            tracing::error!(error = %self, code, "api error");
        } else {
            tracing::debug!(error = %self, code, "api client error");
        }
        let body = json!({
            "data": serde_json::Value::Null,
            "error": { "code": code, "message": self.to_string() }
        });
        (status, Json(body)).into_response()
    }
}

pub type CoreResult<T> = Result<T, CoreError>;

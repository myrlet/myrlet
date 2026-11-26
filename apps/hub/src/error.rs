//! Stable HTTP error mapping and startup configuration failures.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{ErrorResponse, PathError};
use myrlet_store::StoreError;

const UNAUTHORIZED: &str = "invalid or missing bearer token";
const INTERNAL: &str = "the hub could not complete the request";

/// One client-visible failure carrying a stable code and a fixed safe message.
#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Unauthorized", UNAUTHORIZED)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "BadRequest", message)
    }

    pub fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PayloadTooLarge",
            "the request body exceeds the configured limit",
        )
    }

    pub fn request_timeout() -> Self {
        Self::new(
            StatusCode::REQUEST_TIMEOUT,
            "RequestTimeout",
            "the request exceeded the configured timeout",
        )
    }

    pub fn service_unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "ServiceUnavailable",
            "the hub is not accepting new commands",
        )
    }

    pub fn internal() -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", INTERNAL)
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        let (status, code, message) = match error {
            StoreError::NotFound => (
                StatusCode::NOT_FOUND,
                "NotFound",
                "the requested resource does not exist",
            ),
            StoreError::VersionConflict => (
                StatusCode::CONFLICT,
                "VersionConflict",
                "expectedTaskVersion does not match the current task version",
            ),
            StoreError::IdempotencyConflict => (
                StatusCode::CONFLICT,
                "IdempotencyConflict",
                "the Idempotency-Key was reused with a different request",
            ),
            StoreError::InvalidTransition => (
                StatusCode::CONFLICT,
                "InvalidTransition",
                "the task cannot make this transition",
            ),
            StoreError::StaleDelivery => (
                StatusCode::CONFLICT,
                "StaleDelivery",
                "a stale connection epoch or delivery report was rejected",
            ),
            StoreError::InvalidInput(_) => (
                StatusCode::BAD_REQUEST,
                "InvalidInput",
                "the request is not valid for this Task",
            ),
            StoreError::CursorExpired => (
                StatusCode::GONE,
                "CursorExpired",
                "the event cursor is older than the retained window; reload the snapshot",
            ),
            StoreError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized", UNAUTHORIZED),
            internal => {
                tracing::error!(error = %internal, "internal store failure");
                (StatusCode::INTERNAL_SERVER_ERROR, "InternalError", INTERNAL)
            }
        };
        Self::new(status, code, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorResponse {
            error: self.code.to_string(),
            message: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}

/// Startup configuration failure; never echoes secret material.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("MYRLET_ADMIN_TOKEN must be set to a non-empty value")]
    MissingAdminToken,
    #[error("bind address {0:?} is not a valid socket address")]
    InvalidBindAddr(String),
    #[error("{name} must be a non-negative integer, got {value:?}")]
    InvalidNumber { name: String, value: String },
    #[error("{name} must be one of true/false, 1/0, yes/no, on/off, got {value:?}")]
    InvalidBool { name: String, value: String },
    #[error("hub/config.toml is not valid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("hub/credentials.json is not a valid Hub credentials document: {0}")]
    Credentials(#[from] serde_json::Error),
    #[error("stored hubId {0:?} is not a canonical `hub_` identifier")]
    InvalidHubId(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Path(#[from] PathError),
}

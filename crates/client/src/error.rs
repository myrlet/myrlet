//! Client-side error surface: stable codes, retry hints, and HTTP body mapping.

use myrlet_protocol::ErrorResponse;
use thiserror::Error;

/// Everything that can go wrong while talking to a Hub.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClientError {
    #[error("invalid Hub URL: {0}")]
    InvalidUrl(String),
    #[error("Hub returned {status} ({code}): {message}")]
    Http {
        status: u16,
        code: String,
        message: String,
    },
    #[error("event cursor is older than the retained window; reload the snapshot")]
    CursorExpired,
    #[error("Hub rejected the bearer credential")]
    Unauthorized,
    #[error("transport failure: {0}")]
    Transport(String),
    #[error("could not decode a Hub response: {0}")]
    Decode(String),
    #[error("SSE stream exceeded the bounded buffer")]
    StreamOverflow,
    #[error("the request was cancelled")]
    Cancelled,
}

impl ClientError {
    /// Only transport blips, server-side unavailability, and timeouts are worth retrying.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Http { status, .. } => matches!(*status, 408 | 429 | 502 | 503 | 504),
            _ => false,
        }
    }

    /// Map a non-success HTTP response onto a [`ClientError`], preferring the Hub error body.
    pub fn from_http_response(status: u16, body: &[u8]) -> Self {
        if status == 401 {
            return Self::Unauthorized;
        }
        if status == 410 {
            return Self::CursorExpired;
        }
        match serde_json::from_slice::<ErrorResponse>(body) {
            Ok(error) => Self::Http {
                status,
                code: error.error,
                message: error.message,
            },
            Err(_) => Self::Http {
                status,
                code: status_code(status).to_string(),
                message: format!("Hub returned HTTP {status}"),
            },
        }
    }
}

/// Stable code used when the Hub error body is absent or not decodable.
fn status_code(status: u16) -> &'static str {
    match status {
        400 => "BadRequest",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "NotFound",
        409 => "Conflict",
        410 => "CursorExpired",
        413 => "PayloadTooLarge",
        429 => "TooManyRequests",
        500 => "InternalError",
        503 => "Unavailable",
        _ => "HttpError",
    }
}

//! Typed storage errors; the Hub maps them to stable HTTP codes and fixed messages.

use thiserror::Error;

/// Failure from one Hub SQLite operation. Inner SQLite/JSON causes never reach clients.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("record not found")]
    NotFound,
    #[error("expectedTaskVersion does not match the current task version")]
    VersionConflict,
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("illegal state transition")]
    InvalidTransition,
    #[error("the task already has a committed delivery")]
    AlreadyClaimed,
    #[error("stale delivery report or connection epoch")]
    StaleDelivery,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("event cursor is older than the retained window")]
    CursorExpired,
    #[error("unauthorized")]
    Unauthorized,
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Path(#[from] myrlet_protocol::PathError),
}

impl StoreError {
    /// Compact constructor for the store's own validation rules.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

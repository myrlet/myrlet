//! Constant-time `Admin` / `Worker` bearer authentication (`security.md`).

use axum::http::{header::AUTHORIZATION, HeaderMap};
use myrlet_protocol::WorkerId;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::ApiError;
use crate::state::HubState;

const BEARER_PREFIX: &str = "Bearer ";

/// One authenticated caller of an ordinary read route.
///
/// Reads are open to either principal; management routes still demand `Admin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    Admin,
    Worker(WorkerId),
}

/// SHA-256 verifier of `MYRLET_ADMIN_TOKEN`; the plaintext is never retained.
#[derive(Clone)]
pub struct AdminVerifier {
    digest: [u8; 32],
}

impl AdminVerifier {
    /// Hash the operator-provisioned Admin token once at startup.
    pub fn new(token: &str) -> Self {
        Self {
            digest: sha256(token),
        }
    }

    /// Constant-time comparison against the configured Admin token.
    pub fn verify(&self, token: &str) -> bool {
        self.digest.ct_eq(&sha256(token)).into()
    }
}

fn sha256(token: &str) -> [u8; 32] {
    let mut digest = [0u8; 32];
    digest.copy_from_slice(Sha256::digest(token.as_bytes()).as_slice());
    digest
}

/// Read one `Authorization: Bearer <token>` header; empty tokens never authenticate.
pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix(BEARER_PREFIX)?;
    if token.is_empty() || token.contains(char::is_whitespace) {
        return None;
    }
    Some(token)
}

/// Require the Admin principal; Worker credentials can never satisfy this check.
pub fn require_admin(state: &HubState, headers: &HeaderMap) -> Result<(), ApiError> {
    let token = extract_bearer(headers).ok_or_else(ApiError::unauthorized)?;
    if state.admin.verify(token) {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

/// Resolve the Worker principal from its stored SHA-256 verifier in constant time.
pub async fn require_worker(state: &HubState, headers: &HeaderMap) -> Result<WorkerId, ApiError> {
    let token = extract_bearer(headers).ok_or_else(ApiError::unauthorized)?;
    match state.store.authenticate_worker(token.to_string()).await {
        Ok(worker_id) => Ok(worker_id),
        Err(myrlet_store::StoreError::Unauthorized) => Err(ApiError::unauthorized()),
        Err(error) => Err(ApiError::from(error)),
    }
}

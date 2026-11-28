//! Constant-time `Admin` / `Worker` bearer authentication and permission-tier checks
//! (`security.md`).

use axum::http::{header::AUTHORIZATION, HeaderMap};
use myrlet_protocol::{PermissionTier, WorkerId};
use myrlet_store::{AuthenticatedWorker, StoreError};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::ApiError;
use crate::state::HubState;

const BEARER_PREFIX: &str = "Bearer ";

/// One authenticated caller and the authority the Hub grants it.
///
/// The tier always comes from server-controlled state: the Admin token cannot become a
/// Worker principal, and a Worker credential cannot choose its own tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Admin,
    Worker {
        worker_id: WorkerId,
        tier: PermissionTier,
    },
}

impl Caller {
    /// The Worker identity of a Worker principal, or `None` for the Admin principal.
    pub fn worker_id(&self) -> Option<&WorkerId> {
        match self {
            Self::Admin => None,
            Self::Worker { worker_id, .. } => Some(worker_id),
        }
    }

    /// Whether the caller may read global Tasks, Workers, and the event stream.
    pub fn is_global(&self) -> bool {
        match self {
            Self::Admin => true,
            Self::Worker { tier, .. } => tier.is_global(),
        }
    }

    /// The `worker` / `owner` / `admin` label used in authorization errors.
    pub fn tier_label(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Worker { tier, .. } => tier.as_str(),
        }
    }

    /// The Worker identity of a `worker`-tier caller, or `None` for a global caller.
    ///
    /// Global readers never reach Worker-scoped handlers, so `None` here is a
    /// programming error rather than a caller-supplied condition.
    pub fn worker_scope(&self) -> Result<WorkerId, ApiError> {
        self.worker_id().cloned().ok_or_else(ApiError::internal)
    }
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

/// Exact-check the Admin token without touching Worker state.
fn is_admin(state: &HubState, token: &str) -> bool {
    state.admin.verify(token)
}

/// Resolve the Worker principal from its stored SHA-256 verifier in constant time.
///
/// The Admin principal is a distinct identity, so its token is never accepted here even if
/// an operator misconfigured it to equal a stored Worker verifier. This keeps WSS, claims,
/// Progress, Result, and Finish strictly Worker-originated.
pub async fn require_worker(
    state: &HubState,
    headers: &HeaderMap,
) -> Result<AuthenticatedWorker, ApiError> {
    let token = extract_bearer(headers).ok_or_else(ApiError::unauthorized)?;
    if is_admin(state, token) {
        return Err(ApiError::unauthorized());
    }
    match state.store.authenticate_worker(token.to_string()).await {
        Ok(worker) => Ok(worker),
        Err(StoreError::Unauthorized) => Err(ApiError::unauthorized()),
        Err(error) => Err(ApiError::from(error)),
    }
}

/// Resolve either principal. Authentication always runs before any body is read.
pub async fn authenticate(state: &HubState, headers: &HeaderMap) -> Result<Caller, ApiError> {
    let token = extract_bearer(headers).ok_or_else(ApiError::unauthorized)?;
    if is_admin(state, token) {
        return Ok(Caller::Admin);
    }
    let worker = require_worker(state, headers).await?;
    Ok(Caller::Worker {
        worker_id: worker.worker_id,
        tier: worker.permission_tier,
    })
}

/// Require the Admin principal for system and Worker administration.
///
/// A missing or unknown credential is `401`; a valid Worker credential (either tier) is
/// `403`, because the credential authenticates but the tier is insufficient. The Admin
/// token is checked first and never reaches the Worker verifier.
pub async fn require_admin(state: &HubState, headers: &HeaderMap) -> Result<(), ApiError> {
    let token = extract_bearer(headers).ok_or_else(ApiError::unauthorized)?;
    if is_admin(state, token) {
        return Ok(());
    }
    match state.store.authenticate_worker(token.to_string()).await {
        // The Admin principal cannot impersonate a Worker, and no Worker tier may
        // administer the Hub; report the tier that is required and the one presented.
        Ok(worker) => Err(ApiError::forbidden(
            "the admin tier is required",
            worker.permission_tier.as_str(),
            "admin",
        )),
        Err(StoreError::Unauthorized) => Err(ApiError::unauthorized()),
        Err(error) => Err(ApiError::from(error)),
    }
}

/// Require a global reader: the Admin principal or an `owner`-tier Worker.
pub async fn require_global_reader(
    state: &HubState,
    headers: &HeaderMap,
) -> Result<Caller, ApiError> {
    let caller = authenticate(state, headers).await?;
    if caller.is_global() {
        return Ok(caller);
    }
    Err(ApiError::forbidden(
        "the owner or admin tier is required for global reads",
        caller.tier_label(),
        "owner",
    ))
}

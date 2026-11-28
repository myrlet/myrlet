//! Hub-issued Worker credential binding stored at `worker/credentials.json` (`0600`).
//! Missing or corrupt credentials fail closed: a new identity is never generated here.

use std::path::Path;

use myrlet_protocol::paths::{write_private_file_atomic, PathError};
use myrlet_protocol::{load_worker_credentials, CredentialsError as LoadError};
use myrlet_protocol::{ErrorResponse, PairWorkerRequest, PairWorkerResponse, WorkerId};

pub use myrlet_protocol::WorkerCredentials;

/// Failure to load, store, or redeem a Worker credential.
#[derive(Debug, thiserror::Error)]
pub enum CredentialsError {
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("Worker credential could not be serialized")]
    Serialize(#[source] serde_json::Error),
    #[error("pairing with {url} failed: {message}")]
    Pair { url: String, message: String },
    #[error("Hub rejected pairing with status {status}: {message}")]
    PairRejected { status: u16, message: String },
    #[error("Hub returned an unusable pairing response")]
    PairResponse(#[source] serde_json::Error),
    #[error(transparent)]
    Path(#[from] PathError),
}

/// Load the stored binding, verifying `0600`-style ownership first. Never generates one.
pub fn load_credentials(path: &Path) -> Result<WorkerCredentials, CredentialsError> {
    Ok(load_worker_credentials(path)?)
}

/// Store the binding atomically with owner-only permissions.
pub fn save_credentials(
    path: &Path,
    credentials: &WorkerCredentials,
) -> Result<(), CredentialsError> {
    if !credentials.is_complete() {
        return Err(LoadError::Incomplete(path.to_path_buf()).into());
    }
    let bytes = serde_json::to_vec(credentials).map_err(CredentialsError::Serialize)?;
    write_private_file_atomic(path, &bytes)?;
    Ok(())
}

/// Redeem a one-time pairing code (`POST /v1/pair`) and persist the issued binding.
pub async fn pair_with_hub(
    hub_url: &str,
    ca_file: Option<&Path>,
    code: &str,
    worker_id: Option<WorkerId>,
    credentials_path: &Path,
) -> Result<WorkerCredentials, CredentialsError> {
    let http = crate::tls::build_http_client(ca_file).map_err(|error| CredentialsError::Pair {
        url: hub_url.to_owned(),
        message: error.to_string(),
    })?;
    let url = format!("{}/v1/pair", hub_url.trim_end_matches('/'));
    let request = PairWorkerRequest {
        enrollment_code: code.to_owned(),
        worker_id,
    };
    let response = http
        .post(&url)
        .json(&request)
        .send()
        .await
        .map_err(|error| CredentialsError::Pair {
            url: url.clone(),
            message: error.to_string(),
        })?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| CredentialsError::Pair {
            url: url.clone(),
            message: error.to_string(),
        })?;
    if !status.is_success() {
        let message = serde_json::from_slice::<ErrorResponse>(&bytes)
            .map(|error| error.message)
            .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
        return Err(CredentialsError::PairRejected {
            status: status.as_u16(),
            message,
        });
    }
    let paired: PairWorkerResponse =
        serde_json::from_slice(&bytes).map_err(CredentialsError::PairResponse)?;
    let credentials = WorkerCredentials {
        hub_id: paired.hub_id,
        worker_id: paired.worker_id,
        credential: paired.credential,
    };
    if !credentials.is_complete() {
        return Err(LoadError::Incomplete(credentials_path.to_path_buf()).into());
    }
    save_credentials(credentials_path, &credentials)?;
    Ok(credentials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_debug_redacts_the_bearer() {
        let credentials = WorkerCredentials {
            hub_id: "hub_stub".into(),
            worker_id: WorkerId::new("wkr_000000000000"),
            credential: "secret-token".into(),
        };
        let debug = format!("{credentials:?}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(!debug.contains("secret-token"), "{debug}");
    }
}

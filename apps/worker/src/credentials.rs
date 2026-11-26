//! Hub-issued Worker credential binding stored at `worker/credentials.json` (`0600`).
//! Missing or corrupt credentials fail closed: a new identity is never generated here.

use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use myrlet_protocol::paths::{verify_private_file, write_private_file_atomic, PathError};
use myrlet_protocol::{ErrorResponse, PairWorkerRequest, PairWorkerResponse, WorkerId};
use serde::{Deserialize, Serialize};

/// Durable `(hubId, workerId)` binding with the Worker's bearer credential.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerCredentials {
    pub hub_id: String,
    pub worker_id: WorkerId,
    pub credential: String,
}

impl fmt::Debug for WorkerCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkerCredentials")
            .field("hub_id", &self.hub_id)
            .field("worker_id", &self.worker_id)
            .field("credential", &"<redacted>")
            .finish()
    }
}

impl WorkerCredentials {
    /// A complete binding names a Hub, a canonical Worker, and a bearer credential.
    pub fn is_complete(&self) -> bool {
        !self.hub_id.is_empty() && !self.credential.is_empty() && self.worker_id.is_canonical()
    }
}

/// Failure to load, store, or redeem a Worker credential.
#[derive(Debug, thiserror::Error)]
pub enum CredentialsError {
    #[error("no Worker credential at {0:?}; run `myrlet-worker pair` first")]
    Missing(PathBuf),
    #[error("Worker credential at {path:?} is unreadable: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Worker credential at {path:?} is not a valid credential document")]
    Document {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("Worker credential at {0:?} is incomplete or not a canonical worker id")]
    Incomplete(PathBuf),
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
    verify_private_file(path)?;
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(CredentialsError::Missing(path.to_path_buf()))
        }
        Err(source) => {
            return Err(CredentialsError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    let credentials: WorkerCredentials =
        serde_json::from_slice(&bytes).map_err(|source| CredentialsError::Document {
            path: path.to_path_buf(),
            source,
        })?;
    if !credentials.is_complete() {
        return Err(CredentialsError::Incomplete(path.to_path_buf()));
    }
    Ok(credentials)
}

/// Store the binding atomically with owner-only permissions.
pub fn save_credentials(
    path: &Path,
    credentials: &WorkerCredentials,
) -> Result<(), CredentialsError> {
    if !credentials.is_complete() {
        return Err(CredentialsError::Incomplete(path.to_path_buf()));
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
        return Err(CredentialsError::Incomplete(credentials_path.to_path_buf()));
    }
    save_credentials(credentials_path, &credentials)?;
    Ok(credentials)
}

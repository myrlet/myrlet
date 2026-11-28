//! Shared Worker credential document stored at `worker/credentials.json` (`0600`).
//!
//! The Worker daemon and the CLI load the same file, so the document type lives in
//! `protocol` and neither surface invents a second format. Loading fails closed: a
//! missing, corrupt, permissive, or incomplete document never yields a credential.

use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::id::WorkerId;
use crate::paths::{verify_private_file, PathError};

/// Durable `(hubId, workerId)` binding plus the Worker's bearer credential.
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

/// Failure to load the Worker credential document.
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
    #[error(transparent)]
    Path(#[from] PathError),
}

/// Load the stored binding, verifying `0600`-style ownership first. Never generates one.
pub fn load_worker_credentials(path: &Path) -> Result<WorkerCredentials, CredentialsError> {
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

//! Worker local Delivery registry: one atomic JSON file per Delivery plus the
//! persisted inventory revision, under `worker/state/` (`local-runtime.md`).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use myrlet_protocol::delivery::CancelState;
use myrlet_protocol::id::{DeliveryId, ExecutionRef, ExecutorId, SlotId, TaskId};
use myrlet_protocol::paths::{
    ensure_private_dir, verify_private_file, write_private_file_atomic, PathError,
};
use serde::{Deserialize, Serialize};

/// Subdirectory holding one `<delivery_id>.json` file per registered Delivery.
const DELIVERIES_DIR: &str = "deliveries";
/// Persisted inventory revision document.
const INVENTORY_FILE: &str = "inventory.json";

/// One locally retained Delivery: durable correlation, pending report intent, and the
/// last content hashes reconciled with the Hub.
///
/// The Worker is the Delivery's single Progress and Result writer, so the record keeps
/// only "what the Hub last confirmed", never a revision or a history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalDeliveryRecord {
    pub delivery_id: DeliveryId,
    pub task_id: TaskId,
    pub slot_id: SlotId,
    pub executor_id: Option<ExecutorId>,
    /// `worker/progress/<delivery_id>.json`; the Worker never writes this file.
    pub progress_path: PathBuf,
    /// `worker/results/<delivery_id>.md`; the Worker never writes this file either.
    pub result_path: PathBuf,
    pub execution_ref: Option<ExecutionRef>,
    pub finish_pending: bool,
    pub finish_acked: bool,
    /// SHA-256 of the Result text the Hub confirmed; `None` means "Hub holds no Result".
    #[serde(default)]
    pub result_acked_sha256: Option<String>,
    /// Local reading of the Result file could not complete; never treated as absence.
    #[serde(default)]
    pub result_recovery_incomplete: bool,
    /// Backend cancel answer (and optional reason) kept until acknowledged.
    pub cancel_report: Option<(CancelState, Option<String>)>,
    pub cancel_acked: bool,
    /// Highest `observedAtMs` already relayed to Hub for this Delivery.
    pub last_progress_observed_ms: Option<i64>,
    /// SHA-256 of the Progress text the Hub confirmed.
    #[serde(default)]
    pub progress_acked_sha256: Option<String>,
}

impl LocalDeliveryRecord {
    /// A freshly claimed Delivery with nothing executed or reported yet.
    pub fn new(
        delivery_id: DeliveryId,
        task_id: TaskId,
        slot_id: SlotId,
        progress_path: PathBuf,
        result_path: PathBuf,
    ) -> Self {
        Self {
            delivery_id,
            task_id,
            slot_id,
            executor_id: None,
            progress_path,
            result_path,
            execution_ref: None,
            finish_pending: false,
            finish_acked: false,
            result_acked_sha256: None,
            result_recovery_incomplete: false,
            cancel_report: None,
            cancel_acked: false,
            last_progress_observed_ms: None,
            progress_acked_sha256: None,
        }
    }
}

/// Atomic per-Delivery JSON store rooted at `worker/state/`.
#[derive(Debug, Clone)]
pub struct LocalRegistry {
    state_dir: PathBuf,
}

impl LocalRegistry {
    /// Create (or verify) the private state directories. The parent `worker/` directory
    /// must already exist; managed directories are never created recursively.
    pub fn open(state_dir: PathBuf) -> Result<Self, RegistryError> {
        ensure_private_dir(&state_dir)?;
        ensure_private_dir(&state_dir.join(DELIVERIES_DIR))?;
        Ok(Self { state_dir })
    }

    /// Every retained Delivery, ordered by `deliveryId`.
    pub fn load_all(&self) -> Result<Vec<LocalDeliveryRecord>, RegistryError> {
        let dir = self.deliveries_dir();
        let mut records = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|source| io_error(&dir, source))? {
            let path = entry.map_err(|source| io_error(&dir, source))?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            verify_private_file(&path)?;
            let bytes = std::fs::read(&path).map_err(|source| io_error(&path, source))?;
            let record: LocalDeliveryRecord =
                serde_json::from_slice(&bytes).map_err(|source| record_error(&path, source))?;
            records.push(record);
        }
        records.sort_by(|left, right| left.delivery_id.cmp(&right.delivery_id));
        Ok(records)
    }

    /// One retained Delivery, or `None` when this Worker never registered it.
    pub fn get(
        &self,
        delivery_id: &DeliveryId,
    ) -> Result<Option<LocalDeliveryRecord>, RegistryError> {
        let path = self.record_path(delivery_id)?;
        verify_private_file(&path)?;
        match std::fs::read(&path) {
            Ok(bytes) => {
                let record =
                    serde_json::from_slice(&bytes).map_err(|source| record_error(&path, source))?;
                Ok(Some(record))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(source) => Err(io_error(&path, source)),
        }
    }

    /// Durably register a claimed Delivery **before** invoking the Harness. Returns
    /// `false` when the `deliveryId` is already registered, so a replayed claim reply can
    /// never launch a second execution.
    pub fn register_if_absent(&self, record: &LocalDeliveryRecord) -> Result<bool, RegistryError> {
        let path = self.record_path(&record.delivery_id)?;
        let bytes = serde_json::to_vec(record).map_err(|source| record_error(&path, source))?;
        // A synced sibling hard-linked into place is exclusive: the link fails when the
        // record exists, and readers never observe a partial document.
        let temp = self
            .deliveries_dir()
            .join(format!(".record.{}.tmp", uuid::Uuid::new_v4().simple()));
        write_private_file_atomic(&temp, &bytes)?;
        let linked = match std::fs::hard_link(&temp, &path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(source) => Err(io_error(&path, source)),
        };
        let _ = std::fs::remove_file(&temp);
        linked
    }

    /// Atomically replace one Delivery record with an owner-only (`0600`) write.
    pub fn save(&self, record: &LocalDeliveryRecord) -> Result<(), RegistryError> {
        let path = self.record_path(&record.delivery_id)?;
        let bytes = serde_json::to_vec(record).map_err(|source| record_error(&path, source))?;
        write_private_file_atomic(&path, &bytes)?;
        Ok(())
    }

    /// Reuse the persisted inventory revision when the configuration fingerprint is
    /// unchanged, otherwise increment it once.
    pub fn load_or_bump_inventory_revision(&self, fingerprint: &str) -> Result<u64, RegistryError> {
        let path = self.state_dir.join(INVENTORY_FILE);
        verify_private_file(&path)?;
        let current = match std::fs::read(&path) {
            Ok(bytes) => Some(
                serde_json::from_slice::<InventoryRevision>(&bytes)
                    .map_err(|source| record_error(&path, source))?,
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(source) => return Err(io_error(&path, source)),
        };
        let revision = match current {
            Some(current) if current.fingerprint == fingerprint => return Ok(current.revision),
            Some(current) => current.revision.saturating_add(1),
            None => 1,
        };
        let document = InventoryRevision {
            fingerprint: fingerprint.to_owned(),
            revision,
        };
        let bytes = serde_json::to_vec(&document).map_err(|source| record_error(&path, source))?;
        write_private_file_atomic(&path, &bytes)?;
        Ok(revision)
    }

    fn deliveries_dir(&self) -> PathBuf {
        self.state_dir.join(DELIVERIES_DIR)
    }

    fn record_path(&self, delivery_id: &DeliveryId) -> Result<PathBuf, RegistryError> {
        if !delivery_id.is_valid_segment() {
            return Err(RegistryError::InvalidDeliveryId(delivery_id.to_string()));
        }
        Ok(self.deliveries_dir().join(format!("{delivery_id}.json")))
    }
}

/// Persisted inventory revision keyed by a Worker-configuration fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InventoryRevision {
    fingerprint: String,
    revision: u64,
}

fn record_error(path: &Path, source: serde_json::Error) -> RegistryError {
    RegistryError::Record {
        path: path.to_path_buf(),
        source,
    }
}

fn io_error(path: &Path, source: std::io::Error) -> RegistryError {
    RegistryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Failures of the local Delivery registry.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("delivery id {0:?} is not a safe single path segment")]
    InvalidDeliveryId(String),
    #[error("delivery record at {path:?} is not a valid record document")]
    Record {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("registry I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Path(#[from] PathError),
}

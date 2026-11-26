//! `worker/worker.yaml`: the Worker's Slots, capabilities, and Executor Adapters.
//! Only inventory-relevant facts live here; Hub connectivity can be overridden by
//! `MYRLET_HUB_URL`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use myrlet_protocol::inventory::{
    CapabilityClaim, ExecutorInfo, InventoryValidationError, SlotInventory, WorkerInventory,
};
use myrlet_protocol::{ExecutorId, SlotId};
use myrlet_worker::{host_facts, ExecutorAdapter, HarnessCliAdapter, HarnessKind, ProcessAdapter};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

const ENV_HUB_URL: &str = "MYRLET_HUB_URL";
const ENV_SHUTDOWN_TIMEOUT_MS: &str = "MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS";
const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 30_000;

/// Hub endpoint plus the Slots this Worker advertises.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerConfig {
    pub hub_url: String,
    #[serde(default)]
    pub slots: Vec<SlotConfig>,
}

/// One execution/concurrency unit inside this Worker.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotConfig {
    pub slot_id: SlotId,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub advisory_capacity: Option<u32>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<CapabilityClaim>,
    #[serde(default)]
    pub executors: Vec<ExecutorConfig>,
}

/// One operator-configured adapter. `kind` selects the thin Harness bridge.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutorConfig {
    Claude {
        id: ExecutorId,
    },
    Codex {
        id: ExecutorId,
    },
    Pi {
        id: ExecutorId,
    },
    #[serde(rename_all = "camelCase")]
    Process {
        id: ExecutorId,
        program: String,
        #[serde(default)]
        args: Vec<String>,
        timeout_seconds: Option<u64>,
    },
}

impl ExecutorConfig {
    pub fn id(&self) -> &ExecutorId {
        match self {
            Self::Claude { id }
            | Self::Codex { id }
            | Self::Pi { id }
            | Self::Process { id, .. } => id,
        }
    }

    /// Inventory label for this adapter (`claude`, `codex`, `pi`, `process`).
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Claude { .. } => "claude",
            Self::Codex { .. } => "codex",
            Self::Pi { .. } => "pi",
            Self::Process { .. } => "process",
        }
    }

    fn build(&self) -> Arc<dyn ExecutorAdapter> {
        let id = self.id().clone();
        match self {
            Self::Claude { .. } => Arc::new(HarnessCliAdapter::new(id, HarnessKind::Claude)),
            Self::Codex { .. } => Arc::new(HarnessCliAdapter::new(id, HarnessKind::Codex)),
            Self::Pi { .. } => Arc::new(HarnessCliAdapter::new(id, HarnessKind::Pi)),
            Self::Process {
                program,
                args,
                timeout_seconds,
                ..
            } => Arc::new(ProcessAdapter::new(
                id,
                program.clone(),
                args.clone(),
                *timeout_seconds,
            )),
        }
    }
}

impl WorkerConfig {
    /// Parse `worker.yaml`, apply `MYRLET_HUB_URL`, and validate the result.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut config: Self =
            serde_saphyr::from_str(&text).map_err(|error| ConfigError::Parse {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        if let Some(url) =
            std::env::var_os(ENV_HUB_URL).and_then(|value| value.to_str().map(str::to_owned))
        {
            if !url.is_empty() {
                config.hub_url = url;
            }
        }
        config.validate()?;
        Ok(config)
    }

    /// Fail-closed structural checks: usable Hub URL, unique ids, valid inventory.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let url = Url::parse(self.hub_url.trim()).map_err(|error| ConfigError::InvalidHubUrl {
            message: error.to_string(),
        })?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ConfigError::InvalidHubUrl {
                message: "hubUrl must be an http(s) URL with a host".to_owned(),
            });
        }
        let mut slot_ids = HashSet::new();
        for slot in &self.slots {
            if !slot.slot_id.is_valid_segment() {
                return Err(ConfigError::InvalidId {
                    field: "slotId",
                    value: slot.slot_id.to_string(),
                });
            }
            if !slot_ids.insert(&slot.slot_id) {
                return Err(ConfigError::DuplicateSlotId {
                    slot_id: slot.slot_id.to_string(),
                });
            }
            let mut executor_ids = HashSet::new();
            for executor in &slot.executors {
                if !executor.id().is_valid_segment() {
                    return Err(ConfigError::InvalidId {
                        field: "executorId",
                        value: executor.id().to_string(),
                    });
                }
                if !executor_ids.insert(executor.id()) {
                    return Err(ConfigError::DuplicateExecutorId {
                        slot_id: slot.slot_id.to_string(),
                        executor_id: executor.id().to_string(),
                    });
                }
                if let ExecutorConfig::Process { program, .. } = executor {
                    if program.trim().is_empty() {
                        return Err(ConfigError::EmptyProgram {
                            executor_id: executor.id().to_string(),
                        });
                    }
                }
            }
        }
        self.build_inventory(0, 0).validate()?;
        Ok(())
    }

    /// Reportable inventory for `worker.hello` / `worker.inventory`.
    pub fn build_inventory(&self, revision: u64, observed_at_ms: i64) -> WorkerInventory {
        WorkerInventory {
            revision,
            observed_at_ms,
            host: host_facts(),
            slots: self
                .slots
                .iter()
                .map(|slot| SlotInventory {
                    slot_id: slot.slot_id.clone(),
                    enabled: slot.enabled,
                    advisory_capacity: slot.advisory_capacity,
                    roles: slot.roles.clone(),
                    capabilities: slot.capabilities.clone(),
                    executors: slot
                        .executors
                        .iter()
                        .map(|executor| ExecutorInfo {
                            id: executor.id().clone(),
                            kind: executor.kind_label().to_owned(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// SHA-256 over the canonical JSON of the inventory-relevant configuration.
    pub fn inventory_fingerprint(&self) -> String {
        #[derive(Serialize)]
        struct Canonical<'a> {
            slots: &'a [SlotConfig],
        }
        let bytes = serde_json::to_vec(&Canonical { slots: &self.slots }).unwrap_or_default();
        let digest = Sha256::digest(bytes);
        let mut fingerprint = String::with_capacity(digest.len() * 2);
        for &byte in digest.iter() {
            use std::fmt::Write;
            let _ = write!(fingerprint, "{byte:02x}");
        }
        fingerprint
    }

    /// Enabled adapters keyed by `(slotId, executorId)`.
    pub fn build_adapters(&self) -> HashMap<(SlotId, ExecutorId), Arc<dyn ExecutorAdapter>> {
        let mut adapters: HashMap<(SlotId, ExecutorId), Arc<dyn ExecutorAdapter>> = HashMap::new();
        for slot in self.slots.iter().filter(|slot| slot.enabled) {
            for executor in &slot.executors {
                let key = (slot.slot_id.clone(), executor.id().clone());
                adapters.insert(key, executor.build());
            }
        }
        adapters
    }

    /// Executor selected when a `task.offer` omits `executorId`.
    pub fn default_executor(&self, slot_id: &SlotId) -> Option<ExecutorId> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.enabled && &slot.slot_id == slot_id)?;
        slot.executors.first().map(|executor| executor.id().clone())
    }

    /// Graceful shutdown drain bound in milliseconds.
    pub fn shutdown_timeout_ms(&self) -> u64 {
        std::env::var(ENV_SHUTDOWN_TIMEOUT_MS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_SHUTDOWN_TIMEOUT_MS)
    }
}

fn enabled_by_default() -> bool {
    true
}

/// Worker configuration failures.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Worker configuration at {path:?} is unreadable: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Worker configuration at {path:?} is not valid YAML: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("hubUrl is invalid: {message}")]
    InvalidHubUrl { message: String },
    #[error("{field} {value:?} is not a safe identifier")]
    InvalidId { field: &'static str, value: String },
    #[error("duplicate slot id {slot_id:?}")]
    DuplicateSlotId { slot_id: String },
    #[error("slot {slot_id:?} declares duplicate executor id {executor_id:?}")]
    DuplicateExecutorId {
        slot_id: String,
        executor_id: String,
    },
    #[error("executor {executor_id:?} has an empty program")]
    EmptyProgram { executor_id: String },
    #[error(transparent)]
    Inventory(#[from] InventoryValidationError),
}

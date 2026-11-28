//! Hub SQLite persistence: one blocking writer, an append-only event log, typed store calls.

use std::path::Path;

use rusqlite::Connection;

use myrlet_protocol::{ensure_private_dir, verify_sqlite_sidecars, PathError, TaskRecord};

pub mod api_tasks;
pub mod api_workers;
pub mod blocking;
pub mod bundle;
pub mod claim;
pub mod commands;
pub mod deliveries;
pub mod enrollment;
pub mod error;
pub mod events;
pub mod fleet;
pub mod idempotency;
pub mod inventory;
pub mod progress;
pub mod reads;
pub mod reports;
pub mod result;
pub mod schema;
pub mod scoped;
pub mod settings;
pub mod tasks;
pub mod workers;

pub use enrollment::AuthenticatedWorker;
pub use error::StoreError;
pub use myrlet_protocol::WORKER_PROTOCOL_VERSION;

use blocking::BlockingSqlite;

/// Task record, the event emitted when state changed, and whether this was an idempotent replay.
pub type TaskMutation = (TaskRecord, Option<myrlet_protocol::HubEventEnvelope>, bool);

/// Bundle response, its per-Task events, and the idempotent-replay flag.
pub type BundleOutcome = (
    myrlet_protocol::TaskBundleImportResponse,
    Vec<myrlet_protocol::HubEventEnvelope>,
    bool,
);

/// Cancelled Task, the Delivery that needs `task.cancel`, the event, and the replay flag.
pub type CancellationOutcome = (
    TaskRecord,
    Option<myrlet_protocol::DeliveryView>,
    Option<myrlet_protocol::HubEventEnvelope>,
    bool,
);

/// Single-writer Hub SQLite store behind a one-permit async facade.
pub struct HubStore {
    db: BlockingSqlite,
}

impl HubStore {
    /// Open (or create) the Hub database, enforcing private dirs, sidecars, and `0600`.
    pub fn open(db_path: &Path) -> Result<Self, StoreError> {
        let parent = db_path
            .parent()
            .ok_or_else(|| StoreError::invalid("database path has no parent directory"))?;
        ensure_private_dir(parent)?;
        verify_sqlite_sidecars(db_path)?;
        let existed = std::fs::symlink_metadata(db_path).is_ok();
        let connection = Connection::open(db_path)?;
        if !existed {
            restrict_database_mode(db_path)?;
        }
        schema::initialize(&connection)?;
        Ok(Self {
            db: BlockingSqlite::new(connection),
        })
    }

    /// In-memory database with the identical schema, for fast tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        schema::initialize(&connection)?;
        Ok(Self {
            db: BlockingSqlite::new(connection),
        })
    }
}

#[cfg(unix)]
fn restrict_database_mode(db_path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(db_path, std::fs::Permissions::from_mode(0o600)).map_err(
        |source| PathError::Io {
            path: db_path.to_path_buf(),
            source,
        },
    )?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_database_mode(_db_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

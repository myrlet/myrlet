//! Worker row loading, approval derivation, and the fleet event names shared by the
//! enrollment and administration modules.

use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};

use myrlet_protocol::{PermissionTier, WorkerApprovalStatus, WorkerId};

use crate::error::StoreError;
use crate::idempotency::hex;

pub(crate) const CREDENTIAL_BYTES: usize = 32;
pub(crate) const ENROLLMENT_CODE_BYTES: usize = 16;
pub(crate) const DEFAULT_TTL_SECS: u64 = 900;
pub(crate) const MAX_TTL_SECS: u64 = 86_400;

pub const EVENT_WORKER_PAIRED: &str = "worker.paired";
pub const EVENT_WORKER_ROTATED: &str = "worker.rotated";
pub const EVENT_WORKER_REVOKED: &str = "worker.revoked";
pub const EVENT_WORKER_APPROVED: &str = "worker.approved";
pub const EVENT_WORKER_PERMISSION_TIER: &str = "worker.permission_tier";

/// Stored Worker row without the derived Snapshot fields.
///
/// `reported_*` is the latest scheduling report under review; `applied_*` is the snapshot
/// the Hub uses for Slot matching. `revision_high_water` survives a re-pair so an Apply
/// from a previous pairing cannot match a new report.
#[derive(Debug, Clone)]
pub struct WorkerRow {
    pub worker_id: WorkerId,
    pub revoked: bool,
    pub approval: WorkerApprovalStatus,
    /// Server-controlled tier of this Worker principal; never supplied by a client.
    pub permission_tier: PermissionTier,
    pub connection_epoch: u64,
    pub inventory_json: Option<String>,
    pub reported_revision: Option<u64>,
    pub reported_config_json: Option<String>,
    pub applied_revision: Option<u64>,
    pub applied_config_json: Option<String>,
    pub revision_high_water: u64,
    pub last_seen_at_ms: Option<i64>,
}

impl WorkerRow {
    /// Approval as reported to clients: revocation always wins.
    pub fn status(&self) -> WorkerApprovalStatus {
        if self.revoked {
            WorkerApprovalStatus::Revoked
        } else {
            self.approval
        }
    }

    /// Whether the Hub may create a new Delivery for this Worker.
    pub fn may_claim(&self) -> bool {
        self.status() == WorkerApprovalStatus::Approved
            && self.applied_revision.is_some()
            && self.applied_revision == self.reported_revision
    }
}

type RawWorker = (
    String,
    i64,
    String,
    String,
    i64,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<String>,
    i64,
    Option<i64>,
);

fn map_worker(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawWorker> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn parse_approval(raw: &str) -> Result<WorkerApprovalStatus, StoreError> {
    match raw {
        "pending" => Ok(WorkerApprovalStatus::Pending),
        "approved" => Ok(WorkerApprovalStatus::Approved),
        other => Err(StoreError::invalid(format!(
            "unknown stored worker approval {other}"
        ))),
    }
}

/// Parse the stored tier, failing closed on any value outside the Worker tiers.
///
/// A missing or corrupt tier must never widen authority, so it is a storage error
/// rather than a silent fallback to `worker` or `owner`.
pub(crate) fn parse_permission_tier(raw: &str) -> Result<PermissionTier, StoreError> {
    PermissionTier::parse(raw).ok_or_else(|| {
        StoreError::invalid(format!(
            "stored worker permission tier {raw:?} is not worker or owner"
        ))
    })
}

fn parse_revision(value: Option<i64>) -> Result<Option<u64>, StoreError> {
    value
        .map(|raw| {
            u64::try_from(raw)
                .map_err(|_| StoreError::invalid("stored worker revision is negative"))
        })
        .transpose()
}

fn to_worker_row(raw: RawWorker) -> Result<WorkerRow, StoreError> {
    Ok(WorkerRow {
        worker_id: WorkerId::new(raw.0),
        revoked: raw.1 != 0,
        approval: parse_approval(&raw.2)?,
        permission_tier: parse_permission_tier(&raw.3)?,
        connection_epoch: u64::try_from(raw.4)
            .map_err(|_| StoreError::invalid("stored connection epoch is negative"))?,
        inventory_json: raw.5,
        reported_revision: parse_revision(raw.6)?,
        reported_config_json: raw.7,
        applied_revision: parse_revision(raw.8)?,
        applied_config_json: raw.9,
        revision_high_water: u64::try_from(raw.10)
            .map_err(|_| StoreError::invalid("stored revision high-water mark is negative"))?,
        last_seen_at_ms: raw.11,
    })
}

const WORKER_COLUMNS: &str = "worker_id, revoked, approval, permission_tier, connection_epoch, \
     inventory_json, reported_revision, reported_config_json, applied_revision, \
     applied_config_json, revision_high_water, last_seen_at_ms";

pub fn load_worker(conn: &Connection, worker_id: &WorkerId) -> Result<WorkerRow, StoreError> {
    let sql = format!("SELECT {WORKER_COLUMNS} FROM workers WHERE worker_id = ?1");
    let raw = conn
        .query_row(&sql, params![worker_id.as_str()], map_worker)
        .optional()?
        .ok_or(StoreError::NotFound)?;
    to_worker_row(raw)
}

pub(crate) fn random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex(&bytes)
}

pub(crate) fn approval_str(approval: WorkerApprovalStatus) -> &'static str {
    match approval {
        WorkerApprovalStatus::Approved => "approved",
        // `revoked` is derived from its own column and never stored as an approval.
        WorkerApprovalStatus::Pending | WorkerApprovalStatus::Revoked => "pending",
    }
}

/// Every enrolled Worker row, ordered by `workerId`.
pub fn list_worker_rows(conn: &Connection) -> Result<Vec<WorkerRow>, StoreError> {
    let sql = format!("SELECT {WORKER_COLUMNS} FROM workers ORDER BY worker_id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_worker)?;
    let mut workers = Vec::new();
    for row in rows {
        workers.push(to_worker_row(row?)?);
    }
    Ok(workers)
}

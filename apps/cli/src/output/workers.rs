//! Human and `--json` rendering for Worker administration commands.

use std::io::{self, Write};

use myrlet_protocol::{
    CreateEnrollmentCodeResponse, RotateWorkerCredentialResponse, SnapshotWorker, WorkerId,
    WorkerListResponse,
};

use super::{optional, to_json};

/// Print the Worker list.
pub fn worker_list<W: Write>(
    out: &mut W,
    response: &WorkerListResponse,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        return to_json(out, response);
    }
    writeln!(out, "WORKER | ONLINE | REVOKED | DELIVERIES | SLOTS")?;
    for worker in &response.workers {
        writeln!(
            out,
            "{:<20} {:<7} {:<7} {:>10}  {}",
            worker.worker_id,
            worker.online,
            worker.revoked,
            worker.active_deliveries,
            slot_count(worker)
        )?;
    }
    Ok(())
}

/// Print one Worker's detail.
pub fn worker<W: Write>(out: &mut W, worker: &SnapshotWorker, as_json: bool) -> io::Result<()> {
    if as_json {
        return to_json(out, worker);
    }
    writeln!(out, "Worker: {}", worker.worker_id)?;
    writeln!(out, "Online: {}", worker.online)?;
    writeln!(out, "Revoked: {}", worker.revoked)?;
    writeln!(out, "ConnectionEpoch: {}", worker.connection_epoch)?;
    writeln!(out, "ActiveDeliveries: {}", worker.active_deliveries)?;
    writeln!(
        out,
        "LastSeenAtMs: {}",
        optional(worker.last_seen_at_ms.as_ref())
    )?;
    writeln!(out, "Slots: {}", slot_count(worker))
}

fn slot_count(worker: &SnapshotWorker) -> usize {
    worker
        .inventory
        .as_ref()
        .map_or(0, |inventory| inventory.slots.len())
}

/// Print a newly issued enrollment code.
pub fn enrollment<W: Write>(
    out: &mut W,
    response: &CreateEnrollmentCodeResponse,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        return to_json(out, response);
    }
    writeln!(out, "EnrollmentCode: {}", response.enrollment_code)?;
    writeln!(out, "ExpiresAtMs: {}", response.expires_at_ms)?;
    writeln!(
        out,
        "SuggestedWorkerId: {}",
        optional(response.suggested_worker_id.as_ref())
    )
}

/// Print a rotated Worker credential.
pub fn rotate<W: Write>(
    out: &mut W,
    response: &RotateWorkerCredentialResponse,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        return to_json(out, response);
    }
    writeln!(out, "Worker: {}", response.worker_id)?;
    writeln!(out, "Hub: {}", response.hub_id)?;
    writeln!(out, "Credential: {}", response.credential)
}

/// Print a revocation acknowledgement.
pub fn revoked<W: Write>(out: &mut W, worker_id: &WorkerId, as_json: bool) -> io::Result<()> {
    if as_json {
        let payload = serde_json::json!({ "workerId": worker_id, "revoked": true });
        return to_json(out, &payload);
    }
    writeln!(out, "Revoked worker {worker_id}")
}

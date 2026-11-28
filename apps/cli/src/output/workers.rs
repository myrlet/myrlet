//! Human and `--json` rendering for Worker administration commands.

use std::io::{self, Write};

use myrlet_protocol::{
    ApplyWorkerConfigResponse, SnapshotWorker, WorkerApprovalStatus, WorkerDetail,
    WorkerListResponse, WorkerSchedulingConfig,
};

use super::{label, optional, to_json};

fn approval_label(approval: WorkerApprovalStatus) -> &'static str {
    match approval {
        WorkerApprovalStatus::Pending => "pending",
        WorkerApprovalStatus::Approved => "approved",
        WorkerApprovalStatus::Revoked => "revoked",
    }
}

/// Print the Worker list.
pub fn worker_list<W: Write>(
    out: &mut W,
    response: &WorkerListResponse,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        return to_json(out, response);
    }
    writeln!(
        out,
        "WORKER | APPROVAL | ONLINE | DELIVERIES | REPORTED | APPLIED | SLOTS"
    )?;
    for worker in &response.workers {
        writeln!(
            out,
            "{:<20} {:<8} {:<7} {:>10}  {:>8} {:>7}  {}",
            worker.worker_id,
            approval_label(worker.approval),
            worker.online,
            worker.active_deliveries,
            optional(worker.reported_revision.as_ref()),
            optional(worker.applied_revision.as_ref()),
            slot_count(worker)
        )?;
    }
    Ok(())
}

/// Print one Worker's detail, including Slots the Worker could claim for.
pub fn worker_detail<W: Write>(
    out: &mut W,
    detail: &WorkerDetail,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        return to_json(out, detail);
    }
    let worker = &detail.worker;
    writeln!(out, "Worker: {}", worker.worker_id)?;
    writeln!(out, "Approval: {}", approval_label(worker.approval))?;
    writeln!(out, "Online: {}", worker.online)?;
    writeln!(out, "Revoked: {}", worker.revoked)?;
    writeln!(
        out,
        "ReportedRevision: {}",
        optional(worker.reported_revision.as_ref())
    )?;
    writeln!(
        out,
        "AppliedRevision: {}",
        optional(worker.applied_revision.as_ref())
    )?;
    writeln!(out, "ApplyPending: {}", apply_pending(worker))?;
    writeln!(out, "ConnectionEpoch: {}", worker.connection_epoch)?;
    writeln!(out, "ActiveDeliveries: {}", worker.active_deliveries)?;
    writeln!(
        out,
        "LastSeenAtMs: {}",
        optional(worker.last_seen_at_ms.as_ref())
    )?;
    writeln!(out, "Slots: {}", slot_count(worker))?;
    writeln!(
        out,
        "ClaimableTasks: {}",
        if detail.claimable_task_ids.is_empty() {
            "none".to_string()
        } else {
            detail
                .claimable_task_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    )
}

/// Print both scheduling snapshots for review before an Apply.
pub fn worker_config<W: Write>(
    out: &mut W,
    detail: &WorkerDetail,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        return to_json(out, detail);
    }
    let worker = &detail.worker;
    writeln!(out, "Worker: {}", worker.worker_id)?;
    writeln!(out, "Approval: {}", approval_label(worker.approval))?;
    writeln!(
        out,
        "ReportedRevision: {}",
        optional(worker.reported_revision.as_ref())
    )?;
    writeln!(
        out,
        "AppliedRevision: {}",
        optional(worker.applied_revision.as_ref())
    )?;
    writeln!(out, "ApplyPending: {}", apply_pending(worker))?;
    writeln!(out, "ReportedConfig:")?;
    write_config(out, detail.reported_config.as_ref())?;
    writeln!(out, "AppliedConfig:")?;
    write_config(out, worker.applied_config.as_ref())
}

/// Print the revision the Hub now schedules the Worker with.
pub fn config_applied<W: Write>(
    out: &mut W,
    worker_id: &myrlet_protocol::WorkerId,
    response: &ApplyWorkerConfigResponse,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        let payload = serde_json::json!({
            "workerId": worker_id,
            "appliedRevision": response.applied_revision,
        });
        return to_json(out, &payload);
    }
    writeln!(
        out,
        "Applied revision {} for worker {worker_id}",
        response.applied_revision
    )
}

fn write_config<W: Write>(out: &mut W, config: Option<&WorkerSchedulingConfig>) -> io::Result<()> {
    match config {
        None => writeln!(out, "  (none)"),
        Some(config) if config.slots.is_empty() => writeln!(out, "  (no Slots)"),
        Some(config) => {
            for slot in &config.slots {
                writeln!(
                    out,
                    "  {} enabled={} capacity={} roles={} capabilities={} executors={}",
                    slot.slot_id,
                    slot.enabled,
                    optional(slot.advisory_capacity.as_ref()),
                    if slot.roles.is_empty() {
                        "none".to_string()
                    } else {
                        slot.roles.join(",")
                    },
                    if slot.capabilities.is_empty() {
                        "none".to_string()
                    } else {
                        slot.capabilities
                            .iter()
                            .map(|claim| format!("{}:{}", claim.name, label(&claim.state)))
                            .collect::<Vec<_>>()
                            .join(",")
                    },
                    if slot.executors.is_empty() {
                        "none".to_string()
                    } else {
                        slot.executors
                            .iter()
                            .map(|executor| format!("{}:{}", executor.id, executor.kind))
                            .collect::<Vec<_>>()
                            .join(",")
                    }
                )?;
            }
            Ok(())
        }
    }
}

/// Whether the latest report still awaits an Admin Apply.
fn apply_pending(worker: &SnapshotWorker) -> bool {
    worker.reported_revision.is_some() && worker.reported_revision != worker.applied_revision
}

fn slot_count(worker: &SnapshotWorker) -> usize {
    worker
        .applied_config
        .as_ref()
        .map_or(0, |config| config.slots.len())
}

/// Print a newly issued enrollment code (operator bootstrap).
pub fn enrollment<W: Write>(
    out: &mut W,
    response: &myrlet_protocol::CreateEnrollmentCodeResponse,
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
    response: &myrlet_protocol::RotateWorkerCredentialResponse,
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
pub fn revoked<W: Write>(
    out: &mut W,
    worker_id: &myrlet_protocol::WorkerId,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        let payload = serde_json::json!({ "workerId": worker_id, "revoked": true });
        return to_json(out, &payload);
    }
    writeln!(out, "Revoked worker {worker_id}")
}

/// Print an approval acknowledgement.
pub fn approved<W: Write>(
    out: &mut W,
    worker_id: &myrlet_protocol::WorkerId,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        let payload = serde_json::json!({ "workerId": worker_id, "approval": "approved" });
        return to_json(out, &payload);
    }
    writeln!(out, "Approved worker {worker_id}")
}

//! Human and `--json` rendering for every CLI command.

use std::io::{self, Write};

use myrlet_protocol::{
    DeliveryView, GlobalSnapshot, HubEventEnvelope, LocalReply, TaskBundleImportResponse,
    TaskListResponse, TaskRecord, TaskTreeResponse,
};
use serde::Serialize;

mod workers;

pub use workers::{enrollment, revoked, rotate, worker_detail, worker_list};

fn to_json<W: Write, T: Serialize>(out: &mut W, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *out, value).map_err(io::Error::other)?;
    writeln!(out)
}

fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn optional<T: std::fmt::Display>(value: Option<&T>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

fn list<T: std::fmt::Display>(values: &[T]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(width.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

fn delivery_summary(delivery: &DeliveryView) -> String {
    if delivery.progress.is_some() {
        format!("{} +progress", label(&delivery.state))
    } else {
        label(&delivery.state)
    }
}

/// Print one authoritative Task record.
pub fn task_record<W: Write>(out: &mut W, record: &TaskRecord, as_json: bool) -> io::Result<()> {
    if as_json {
        return to_json(out, record);
    }
    writeln!(out, "Task: {}", record.id)?;
    writeln!(out, "Title: {}", record.task.title)?;
    writeln!(out, "Status: {}", label(&record.status))?;
    writeln!(out, "Version: {}", record.version)?;
    writeln!(out, "Root: {}", record.root_task_id)?;
    writeln!(
        out,
        "Parent: {}",
        optional(record.task.parent_task_id.as_ref())
    )?;
    writeln!(out, "Depth: {}", record.depth)?;
    writeln!(out, "Dependencies: {}", list(&record.task.dependencies))?;
    if record.blocked_by.is_empty() {
        writeln!(out, "BlockedBy: none")?;
    } else {
        writeln!(out, "BlockedBy:")?;
        for blocker in &record.blocked_by {
            writeln!(out, "  {} ({})", blocker.task_id, label(&blocker.status))?;
        }
    }
    match &record.current_delivery {
        None => writeln!(out, "Delivery: none")?,
        Some(delivery) => {
            writeln!(out, "Delivery: {}", delivery.id)?;
            writeln!(out, "  State: {}", label(&delivery.state))?;
            writeln!(out, "  Worker: {}", delivery.worker_id)?;
            writeln!(out, "  Slot: {}", delivery.slot_id)?;
            let cancel_state = delivery
                .cancel_state
                .as_ref()
                .map_or_else(|| "none".to_string(), label);
            writeln!(out, "  CancelState: {cancel_state}")?;
            if let Some(progress) = &delivery.progress {
                writeln!(out, "Progress:")?;
                writeln!(
                    out,
                    "  FirstReceivedAtMs: {}",
                    progress.first_received_at_ms
                )?;
                writeln!(out, "  ReceivedAtMs: {}", progress.received_at_ms)?;
                writeln!(out, "  Text: {}", progress.text)?;
            }
        }
    }
    if let Some(result) = &record.result_markdown {
        writeln!(out, "Result:")?;
        if let Some(recorded_at_ms) = record.result_recorded_at_ms {
            writeln!(out, "  RecordedAtMs: {recorded_at_ms}")?;
        }
        for line in result.lines() {
            writeln!(out, "  {line}")?;
        }
    }
    Ok(())
}

/// Print the bounded Task summary page.
pub fn task_list<W: Write>(out: &mut W, page: &TaskListResponse, as_json: bool) -> io::Result<()> {
    if as_json {
        return to_json(out, page);
    }
    writeln!(out, "ID | STATUS | VER | TITLE | DELIVERY")?;
    for task in &page.tasks {
        let delivery = task
            .current_delivery
            .as_ref()
            .map_or_else(|| "-".to_string(), delivery_summary);
        let blocked = if task.blocked_by.is_empty() {
            String::new()
        } else {
            format!(" blocked:{}", task.blocked_by.len())
        };
        writeln!(
            out,
            "{:<36} {:<10} {:>4}  {:<24} {}{}",
            task.id,
            label(&task.status),
            task.version,
            truncate(&task.title, 24),
            delivery,
            blocked
        )?;
    }
    writeln!(
        out,
        "total {} task(s), truncated: {}",
        page.total_tasks, page.truncated
    )
}

/// Print a bounded Task tree.
pub fn task_tree<W: Write>(out: &mut W, tree: &TaskTreeResponse, as_json: bool) -> io::Result<()> {
    if as_json {
        return to_json(out, tree);
    }
    writeln!(out, "Root: {}", tree.root_task_id)?;
    for task in &tree.tasks {
        let indent = "  ".repeat(task.depth.saturating_sub(1) as usize);
        writeln!(
            out,
            "{indent}{}  {}  v{}  {}",
            task.id,
            label(&task.status),
            task.version,
            task.task.title
        )?;
    }
    writeln!(out, "truncated: {}", tree.truncated)
}

/// Print a Bundle import response.
pub fn bundle_import<W: Write>(
    out: &mut W,
    response: &TaskBundleImportResponse,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        return to_json(out, response);
    }
    writeln!(out, "Imported {} task(s)", response.tasks.len())?;
    writeln!(out, "Eligible now: {}", list(&response.eligible_task_ids))?;
    for (client_ref, id) in &response.client_ref_to_task_id {
        let status = response
            .tasks
            .iter()
            .find(|task| &task.id == id)
            .map(|task| label(&task.status))
            .unwrap_or_default();
        writeln!(out, "  {client_ref} -> {id} ({status})")?;
    }
    Ok(())
}

/// Print the result of one execution-local command.
pub fn local_reply<W: Write>(out: &mut W, reply: &LocalReply, as_json: bool) -> io::Result<()> {
    if as_json {
        return to_json(out, reply);
    }
    writeln!(out, "Operation: {}", reply.operation)?;
    writeln!(out, "Delivery: {}", reply.delivery_id)?;
    writeln!(out, "LocalReceipt: {}", reply.local_receipt)?;
    if let Some(confirmed) = reply.hub_confirmed {
        writeln!(out, "HubConfirmed: {confirmed}")?;
    }
    writeln!(out, "Outcome: {}", reply.outcome)?;
    if let Some(local) = &reply.local_sha256 {
        writeln!(out, "LocalSha256: {local}")?;
    }
    if let Some(hub) = &reply.hub_sha256 {
        writeln!(out, "HubSha256: {hub}")?;
    }
    for warning in &reply.warnings {
        writeln!(out, "Warning: {warning}")?;
    }
    Ok(())
}

/// Print one NDJSON Snapshot line for `events watch`.
pub fn snapshot_line<W: Write>(
    out: &mut W,
    snapshot: &GlobalSnapshot,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        let payload = serde_json::json!({ "type": "snapshot", "snapshot": snapshot });
        return to_json(out, &payload);
    }
    writeln!(
        out,
        "snapshot: {} task(s), {} worker(s), eventCursor {}",
        snapshot.total_tasks,
        snapshot.workers.len(),
        snapshot.event_cursor
    )
}

/// Print one NDJSON event line for `events watch`.
pub fn event_line<W: Write>(
    out: &mut W,
    event: &HubEventEnvelope,
    as_json: bool,
) -> io::Result<()> {
    if as_json {
        let payload = serde_json::json!({ "type": "event", "event": event });
        return to_json(out, &payload);
    }
    writeln!(
        out,
        "event {} {} at {}",
        event.seq, event.kind, event.created_at_ms
    )
}

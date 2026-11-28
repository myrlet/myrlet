//! Worker claims: atomic eligibility recheck plus the single committed Delivery.

use rusqlite::{params, Connection};

use myrlet_protocol::{
    ClaimTaskRequest, ClaimTaskResponse, DeliveryId, DeliveryStatus, ExecutorId, HubEventEnvelope,
    SlotId, TaskDefinition, TaskStatus, WorkerId,
};

use crate::deliveries::{current_delivery, delivery_view, load_delivery};
use crate::error::StoreError;
use crate::events::insert_event;
use crate::tasks::{
    are_dependencies_satisfied, load_task, status_str, transition_task, EVENT_TASK_DISPATCHED,
};
use crate::workers::load_worker;

/// Commit one claim: the Hub rechecks readiness, ownership, and Slot eligibility, then
/// creates the Task's unique Delivery in the same transaction.
///
/// A competing claim for the same Task fails on the unique Delivery index or on the
/// version check, so exactly one winner commits.
pub fn claim_task(
    conn: &mut Connection,
    worker_id: &WorkerId,
    request: &ClaimTaskRequest,
    now_ms: i64,
) -> Result<(ClaimTaskResponse, HubEventEnvelope), StoreError> {
    if request.connection_epoch == 0 {
        return Err(StoreError::StaleDelivery);
    }
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.revoked {
        return Err(StoreError::Unauthorized);
    }
    // A claim always belongs to the live connection: an older epoch cannot claim.
    if worker.connection_epoch != request.connection_epoch {
        return Err(StoreError::StaleDelivery);
    }
    let row = load_task(&tx, &request.task_id)?;
    if row.status != TaskStatus::Queued || !are_dependencies_satisfied(&tx, &request.task_id)? {
        return Err(StoreError::InvalidTransition);
    }
    if current_delivery(&tx, &request.task_id)?.is_some() {
        return Err(StoreError::AlreadyClaimed);
    }
    let definition: TaskDefinition = row.definition()?;
    // The Task's preferred Executor is a soft preference that only matters at launch: the
    // Hub resolves it against the claimed Slot, so the Worker need not know the definition.
    let requested = request
        .executor_id
        .clone()
        .or_else(|| definition.routing.preferred_executor_id.clone());
    let slot = claimable_slot(
        &worker.inventory_json,
        &request.slot_id,
        &definition,
        requested.as_ref(),
    )?;
    let executor_id = requested
        .filter(|id| slot.executors.iter().any(|executor| &executor.id == id))
        .or_else(|| slot.executors.first().map(|executor| executor.id.clone()));
    let delivery_id = DeliveryId::new(format!("delivery_{}", uuid::Uuid::new_v4().simple()));
    tx.execute(
        "INSERT INTO deliveries (id, task_id, worker_id, slot_id, executor_id, connection_epoch, \
         state, cancel_state, cancel_reason, created_at_ms, updated_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'accepted', NULL, NULL, ?7, ?7)",
        params![
            delivery_id.as_str(),
            request.task_id.as_str(),
            worker_id.as_str(),
            request.slot_id.as_str(),
            executor_id.as_ref().map(ExecutorId::as_str),
            request.connection_epoch as i64,
            now_ms
        ],
    )?;
    let version = transition_task(&tx, &row, TaskStatus::Dispatched, now_ms)?;
    let delivery = delivery_view(&tx, &load_delivery(&tx, &delivery_id)?)?;
    let event = insert_event(
        &tx,
        EVENT_TASK_DISPATCHED,
        &serde_json::json!({
            "taskId": request.task_id.as_str(),
            "deliveryId": delivery_id.as_str(),
            "workerId": worker_id.as_str(),
            "slotId": request.slot_id.as_str(),
            "status": status_str(TaskStatus::Dispatched),
            "version": version,
        }),
        now_ms,
    )?;
    let task = crate::tasks::build_record(&tx, &load_task(&tx, &request.task_id)?)?;
    tx.commit()?;
    Ok((ClaimTaskResponse { task, delivery }, event))
}

/// Return the claimed Slot only when it is enabled and satisfies the Task's hard rules.
fn claimable_slot(
    inventory_json: &Option<String>,
    slot_id: &SlotId,
    definition: &TaskDefinition,
    executor_id: Option<&ExecutorId>,
) -> Result<myrlet_protocol::SlotInventory, StoreError> {
    let inventory: Option<myrlet_protocol::WorkerInventory> = match inventory_json {
        Some(raw) => Some(serde_json::from_str(raw)?),
        None => None,
    };
    let slot = inventory
        .as_ref()
        .and_then(|inventory| inventory.slots.iter().find(|slot| &slot.slot_id == slot_id))
        .ok_or_else(|| StoreError::invalid("the reported Slot does not exist"))?
        .clone();
    if !slot.accepts(&definition.routing, executor_id) {
        return Err(StoreError::invalid(
            "the Slot does not satisfy this Task's capabilities or Executor",
        ));
    }
    Ok(slot)
}

/// Whether a Delivery still admits Progress and Result reconciliation.
pub fn accepts_reports(state: DeliveryStatus) -> bool {
    matches!(
        state,
        DeliveryStatus::Accepted | DeliveryStatus::Finished | DeliveryStatus::Cancelled
    )
}

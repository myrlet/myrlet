//! Pure deterministic Targeted and Simple Match resolution (`task-model.md`).

use std::cmp::Reverse;

use myrlet_protocol::{
    CapabilityState, DispatchSelection, ExecutorId, SlotId, SlotInventory, SnapshotWorker,
    TaskRouting, WorkerId,
};
use myrlet_store::DeliveryTarget;

/// Ascending rank key: the smallest key wins every documented tie-breaker.
type RankKey = (u8, u8, u8, u64, (bool, Reverse<u32>), WorkerId, SlotId);

/// Resolve one dispatch command into a concrete Delivery target, if eligible.
pub fn resolve(
    workers: &[SnapshotWorker],
    routing: &TaskRouting,
    selection: &DispatchSelection,
) -> Option<DeliveryTarget> {
    match selection {
        DispatchSelection::Targeted {
            worker_id,
            slot_id,
            executor_id,
        } => targeted(workers, worker_id, slot_id, executor_id.as_ref()),
        DispatchSelection::Match { fallback_allowed } => {
            best_match(workers, routing, *fallback_allowed)
        }
    }
}

fn targeted(
    workers: &[SnapshotWorker],
    worker_id: &WorkerId,
    slot_id: &SlotId,
    executor_id: Option<&ExecutorId>,
) -> Option<DeliveryTarget> {
    let worker = workers
        .iter()
        .find(|worker| &worker.worker_id == worker_id)?;
    if !worker.online || worker.revoked {
        return None;
    }
    let slot = worker
        .inventory
        .as_ref()?
        .slots
        .iter()
        .find(|slot| &slot.slot_id == slot_id)?;
    if !slot.enabled || executor_id.is_some_and(|id| !has_executor(slot, id)) {
        return None;
    }
    Some(target_for(worker, slot, executor_id.cloned()))
}

fn best_match(
    workers: &[SnapshotWorker],
    routing: &TaskRouting,
    fallback_allowed: bool,
) -> Option<DeliveryTarget> {
    let only = routing
        .preferred_worker_id
        .as_ref()
        .filter(|_| !fallback_allowed);
    let mut best: Option<(RankKey, DeliveryTarget)> = None;
    for worker in workers {
        let offline = !worker.online || worker.revoked;
        if offline || only.is_some_and(|only| only != &worker.worker_id) {
            continue;
        }
        let Some(inventory) = worker.inventory.as_ref() else {
            continue;
        };
        for slot in &inventory.slots {
            if !slot.enabled || !capabilities_available(slot, routing) {
                continue;
            }
            let key = rank_key(worker, slot, routing);
            if best.as_ref().is_none_or(|(best, _)| key < *best) {
                let executor_id = pick_executor(slot, routing);
                best = Some((key, target_for(worker, slot, executor_id)));
            }
        }
    }
    best.map(|(_, target)| target)
}

fn capabilities_available(slot: &SlotInventory, routing: &TaskRouting) -> bool {
    routing.required_capabilities.iter().all(|required| {
        slot.capabilities
            .iter()
            .any(|claim| claim.name == *required && claim.state == CapabilityState::Available)
    })
}

fn rank_key(worker: &SnapshotWorker, slot: &SlotInventory, routing: &TaskRouting) -> RankKey {
    let wrong_worker = u8::from(
        routing
            .preferred_worker_id
            .as_ref()
            .is_some_and(|id| id != &worker.worker_id),
    );
    let wrong_role = u8::from(
        routing
            .preferred_role
            .as_ref()
            .is_some_and(|role| !slot.roles.contains(role)),
    );
    let wrong_executor = u8::from(
        routing
            .preferred_executor_id
            .as_ref()
            .is_some_and(|id| !has_executor(slot, id)),
    );
    let capacity = slot.advisory_capacity;
    (
        wrong_worker,
        wrong_role,
        wrong_executor,
        worker.active_deliveries,
        (capacity.is_none(), Reverse(capacity.unwrap_or(0))),
        worker.worker_id.clone(),
        slot.slot_id.clone(),
    )
}

fn has_executor(slot: &SlotInventory, executor_id: &ExecutorId) -> bool {
    slot.executors
        .iter()
        .any(|executor| &executor.id == executor_id)
}

fn pick_executor(slot: &SlotInventory, routing: &TaskRouting) -> Option<ExecutorId> {
    match &routing.preferred_executor_id {
        Some(preferred) if has_executor(slot, preferred) => Some(preferred.clone()),
        _ => slot.executors.first().map(|executor| executor.id.clone()),
    }
}

fn target_for(
    worker: &SnapshotWorker,
    slot: &SlotInventory,
    executor_id: Option<ExecutorId>,
) -> DeliveryTarget {
    DeliveryTarget {
        worker_id: worker.worker_id.clone(),
        slot_id: slot.slot_id.clone(),
        executor_id,
        connection_epoch: worker.connection_epoch,
    }
}

//! Notify one eligible Worker at a time about each scanned queued Task.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::state::{now_ms, HubState};
use crate::ws::build_hub_frame;
use myrlet_protocol::{
    HubWakeupPayload, SlotId, SnapshotWorker, TaskDefinition, TaskId, WorkerId, WorkerWireKind,
};

const WAKEUP_SCAN_LIMIT: usize = 100;
const WAKEUP_INTERVAL: Duration = Duration::from_secs(1);

/// One in-memory cursor per scanned Task. The lock also serializes wakeup passes.
pub(crate) type WakeupRotation = tokio::sync::Mutex<HashMap<TaskId, (WorkerId, Instant)>>;

async fn wake_eligible(state: &HubState) {
    let mut rotation = state.wakeup_rotation.lock().await;
    let queued = match state.store.list_queued_tasks(WAKEUP_SCAN_LIMIT).await {
        Ok(queued) => queued,
        Err(error) => {
            tracing::warn!(%error, "could not scan queued tasks");
            return;
        }
    };
    rotation.retain(|id, _| queued.iter().any(|(queued_id, _)| queued_id == id));
    if queued.is_empty() {
        return;
    }
    let mut workers = match state.store.list_workers(state.online_map()).await {
        Ok(workers) => workers,
        Err(error) => {
            tracing::warn!(%error, "could not read worker inventory");
            return;
        }
    };
    workers.retain(|worker| worker.online && !worker.revoked);
    workers.sort_unstable_by(|a, b| a.worker_id.as_str().cmp(b.worker_id.as_str()));
    for (task_id, definition) in queued {
        if state.is_shutting_down() {
            return;
        }
        let last = rotation.get(&task_id);
        if last.is_some_and(|(_, sent)| sent.elapsed() < WAKEUP_INTERVAL) {
            continue;
        }
        let eligible: Vec<_> = workers
            .iter()
            .filter_map(|worker| eligible_slots(worker, &definition).map(|slots| (worker, slots)))
            .collect();
        let next = match last {
            Some((id, _)) => eligible
                .iter()
                .position(|(worker, _)| worker.worker_id.as_str() > id.as_str()),
            None => eligible.iter().position(|(worker, _)| {
                definition.routing.preferred_worker_id.as_ref() == Some(&worker.worker_id)
            }),
        };
        let Some((worker, eligible_slot_ids)) = eligible.into_iter().nth(next.unwrap_or(0)) else {
            continue;
        };
        let payload = HubWakeupPayload {
            task_id: task_id.clone(),
            eligible_slot_ids,
        };
        if let Some(frame) = build_hub_frame(
            WorkerWireKind::HubWakeup,
            &worker.worker_id,
            None,
            worker.connection_epoch,
            now_ms(),
            payload,
        ) {
            let _ = state.send_to_worker(&worker.worker_id, worker.connection_epoch, frame);
            rotation.insert(task_id, (worker.worker_id.clone(), Instant::now()));
        }
    }
}

pub(crate) fn spawn_wakeup(state: &HubState) {
    let state = state.clone();
    tokio::spawn(async move { wake_eligible(&state).await });
}

/// Retry unanswered hints while their Tasks remain queued.
#[doc(hidden)]
pub fn spawn_wakeup_loop(state: &HubState) {
    let state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(WAKEUP_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if state.is_shutting_down() {
                return;
            }
            wake_eligible(&state).await;
        }
    });
}

fn eligible_slots(worker: &SnapshotWorker, definition: &TaskDefinition) -> Option<Vec<SlotId>> {
    let inventory = worker.inventory.as_ref()?;
    let slots: Vec<_> = inventory
        .slots
        .iter()
        .filter(|slot| !slot.executors.is_empty() && slot.accepts(&definition.routing, None))
        .map(|slot| slot.slot_id.clone())
        .collect();
    (!slots.is_empty()).then_some(slots)
}

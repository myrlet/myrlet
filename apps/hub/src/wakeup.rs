//! Wakeup hints: tell one eligible Worker about one `queued` Task.
//!
//! A wakeup is a hint only. It reserves no Slot, creates no Delivery, and never retries
//! by itself. The Worker decides whether to claim; a silent Worker simply stays silent and
//! the Hub may wake another eligible Worker on the next state change.

use crate::state::{now_ms, HubState};
use crate::ws::build_hub_frame;
use myrlet_protocol::{HubWakeupPayload, SlotInventory, TaskDefinition, WorkerWireKind};

/// Queued Tasks inspected per wakeup pass.
const WAKEUP_SCAN_LIMIT: usize = 100;

/// Wake one eligible Worker for each `queued` Task that no Worker has claimed.
pub async fn wake_eligible(state: &HubState) {
    let queued = match state.store.list_queued_tasks(WAKEUP_SCAN_LIMIT).await {
        Ok(queued) => queued,
        Err(error) => {
            tracing::warn!(%error, "could not scan queued tasks");
            return;
        }
    };
    if queued.is_empty() {
        return;
    }
    let workers = match state.store.list_workers(state.online_map()).await {
        Ok(workers) => workers,
        Err(error) => {
            tracing::warn!(%error, "could not read worker inventory");
            return;
        }
    };
    for (task_id, definition) in queued {
        if state.is_shutting_down() {
            return;
        }
        let Some(worker) = select_worker(&workers, &definition) else {
            continue;
        };
        let payload = HubWakeupPayload {
            task_id: task_id.clone(),
        };
        let frame = build_hub_frame(
            WorkerWireKind::HubWakeup,
            &worker.worker_id,
            None,
            worker.connection_epoch,
            now_ms(),
            payload,
        );
        if let Some(frame) = frame {
            let _ = state.send_to_worker(&worker.worker_id, worker.connection_epoch, frame);
        }
    }
}

/// Run one wakeup pass on the current runtime after a state change.
pub fn spawn_wakeup(state: &HubState) {
    let state = state.clone();
    tokio::spawn(async move { wake_eligible(&state).await });
}

/// Pick one online Worker with an enabled Slot that meets the Task's hard requirements.
///
/// The choice is deterministic and a hint only: a preferred Worker first, then the least
/// busy, then `workerId`. Preferences never block a wakeup; the Worker still decides.
fn select_worker<'a>(
    workers: &'a [myrlet_protocol::SnapshotWorker],
    definition: &TaskDefinition,
) -> Option<&'a myrlet_protocol::SnapshotWorker> {
    let preferred = definition.routing.preferred_worker_id.as_ref();
    workers
        .iter()
        .filter(|worker| worker.online && !worker.revoked)
        .filter(|worker| has_claimable_slot(worker, definition))
        .min_by_key(|worker| {
            let preferred_wrong = u8::from(preferred.is_some_and(|id| id != &worker.worker_id));
            (
                preferred_wrong,
                worker.active_deliveries,
                worker.worker_id.clone(),
            )
        })
}

/// Whether an enabled Slot meets the Task's hard routing rules. Advisory capacity is
/// never consulted here: it cannot make a Worker ineligible.
fn has_claimable_slot(
    worker: &myrlet_protocol::SnapshotWorker,
    definition: &TaskDefinition,
) -> bool {
    let Some(inventory) = worker.inventory.as_ref() else {
        return false;
    };
    let slots: &[SlotInventory] = &inventory.slots;
    slots
        .iter()
        .any(|slot| slot.accepts(&definition.routing, None))
}

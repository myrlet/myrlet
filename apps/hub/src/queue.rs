//! Simple FIFO queue drain and the shared offer delivery path with pre-send release.

use myrlet_protocol::{DeliveryView, TaskDefinition, TaskOfferPayload, WorkerWireKind};

use crate::matcher;
use crate::state::{now_ms, HubState};
use crate::ws::build_hub_frame;

/// Queued Tasks inspected per drain pass (`task-model.md` FIFO scan bound).
const QUEUE_SCAN_LIMIT: usize = 100;

/// Offer every queued Task whose dependencies and target requirements are satisfied.
pub async fn drain_fifo_queue(state: &HubState) {
    let queued = match state.store.list_queued_tasks(QUEUE_SCAN_LIMIT).await {
        Ok(queued) => queued,
        Err(error) => {
            tracing::warn!(%error, "could not scan the FIFO queue");
            return;
        }
    };
    for (task_id, version, definition, request) in queued {
        if state.is_shutting_down() {
            return;
        }
        let ready = state
            .store
            .are_dependencies_satisfied(task_id.clone())
            .await
            .unwrap_or(false);
        if !ready {
            continue;
        }
        let workers = match state.store.list_workers(state.online_map()).await {
            Ok(workers) => workers,
            Err(error) => {
                tracing::warn!(%error, "could not read worker inventory");
                return;
            }
        };
        let routing = &definition.routing;
        let Some(target) = matcher::resolve(&workers, routing, &request.selection) else {
            continue;
        };
        let created = state
            .store
            .create_delivery(None, task_id.clone(), version, target, now_ms())
            .await;
        match created {
            Ok((response, event, _)) => {
                state.publish_event(event);
                send_offer_or_release(state, response.delivery, &definition, true).await;
            }
            Err(error) => tracing::debug!(%error, "queued task is not dispatchable right now"),
        }
    }
}

/// Run one drain pass on the current runtime after a state change frees capacity.
pub fn spawn_drain(state: &HubState) {
    let state = state.clone();
    tokio::spawn(async move { drain_fifo_queue(&state).await });
}

/// Send one committed `task.offer`; a failed hand-off releases the Delivery back to waiting.
pub async fn send_offer_or_release(
    state: &HubState,
    delivery: Option<DeliveryView>,
    definition: &TaskDefinition,
    requeue_on_fail: bool,
) {
    let Some(delivery) = delivery else {
        return;
    };
    let payload = TaskOfferPayload {
        task_id: delivery.task_id.clone(),
        delivery_id: delivery.id.clone(),
        slot_id: delivery.slot_id.clone(),
        executor_id: delivery.executor_id.clone(),
        task: definition.clone(),
    };
    let frame = build_hub_frame(
        WorkerWireKind::TaskOffer,
        &delivery.worker_id,
        None,
        delivery.connection_epoch,
        now_ms(),
        payload,
    );
    if let Some(frame) = frame {
        if state.send_to_worker(&delivery.worker_id, delivery.connection_epoch, frame) {
            return;
        }
    }
    let released = state
        .store
        .release_delivery(delivery.id, requeue_on_fail, now_ms())
        .await;
    match released {
        Ok(event) => state.publish_event(event),
        Err(error) => tracing::warn!(%error, "could not release an unsent delivery"),
    }
}

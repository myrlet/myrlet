//! Inbound Worker WSS frames: heartbeats, inventory, cancel reports, and resync.

use axum::extract::ws::WebSocket;
use myrlet_protocol::{
    DeliveryId, HubHeartbeatAckPayload, TaskCancelPayload, TaskCancelledAckPayload,
    TaskCancelledPayload, WorkerId, WorkerInventory, WorkerInventoryAckPayload,
    WorkerResyncPayload, WorkerWireEnvelope, WorkerWireKind, WORKER_PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;

use super::send_frame;
use crate::state::{now_ms, HubState};
use crate::wakeup::spawn_wakeup;

/// Outcome of handling one inbound Worker WSS frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameOutcome {
    Accepted,
    Ignored,
    Close,
}

/// Apply one authenticated, current-epoch Worker frame.
pub(super) async fn handle_frame(
    state: &HubState,
    socket: &mut WebSocket,
    worker_id: &WorkerId,
    connection_id: &str,
    epoch: u64,
    text: &str,
) -> FrameOutcome {
    let Ok(frame) = serde_json::from_str::<WorkerWireEnvelope>(text) else {
        return FrameOutcome::Ignored;
    };
    let fenced = frame.v != WORKER_PROTOCOL_VERSION
        || &frame.worker_id != worker_id
        || frame.connection_epoch.is_some_and(|value| value != epoch);
    if fenced {
        return FrameOutcome::Ignored; // a stale or foreign frame never mutates Hub state
    }
    let now = now_ms();
    let connection = Session {
        state,
        socket,
        worker_id,
        connection_id,
        epoch,
    };
    if connection.dispatch(frame.kind, frame.payload, now).await {
        FrameOutcome::Accepted
    } else {
        FrameOutcome::Close
    }
}

/// One connection's reply channel plus its fencing identity.
struct Session<'a> {
    state: &'a HubState,
    socket: &'a mut WebSocket,
    worker_id: &'a WorkerId,
    connection_id: &'a str,
    epoch: u64,
}

impl Session<'_> {
    async fn dispatch(self, kind: WorkerWireKind, payload: serde_json::Value, now: i64) -> bool {
        match kind {
            WorkerWireKind::WorkerHeartbeat => self.heartbeat(now).await,
            WorkerWireKind::WorkerInventory => self.inventory(payload, now).await,
            WorkerWireKind::TaskCancelled => self.cancelled(payload, now).await,
            WorkerWireKind::WorkerResync => self.resync(payload, now).await,
            WorkerWireKind::WorkerBye => false,
            _ => true,
        }
    }

    async fn reply<T: serde::Serialize>(&mut self, kind: WorkerWireKind, payload: T) -> bool {
        send_frame(
            self.socket,
            kind,
            self.worker_id,
            self.connection_id,
            self.epoch,
            payload,
        )
        .await
    }

    async fn heartbeat(mut self, now: i64) -> bool {
        let seen = self
            .state
            .store
            .touch_worker_seen(self.worker_id.clone(), self.epoch, now)
            .await;
        if seen.is_err() {
            tracing::debug!("worker heartbeat could not be recorded");
        }
        self.reply(
            WorkerWireKind::HubHeartbeatAck,
            HubHeartbeatAckPayload {
                server_time_ms: now,
            },
        )
        .await;
        true
    }

    async fn inventory(mut self, payload: serde_json::Value, now: i64) -> bool {
        let Some(inventory) = decode::<WorkerInventory>(payload) else {
            return true;
        };
        let update = self
            .state
            .store
            .update_worker_inventory(self.worker_id.clone(), self.epoch, inventory.clone(), now)
            .await;
        let (accepted, events) = update.unwrap_or((false, Vec::new()));
        for event in events {
            self.state.publish_event(Some(event));
        }
        self.reply(
            WorkerWireKind::WorkerInventoryAck,
            WorkerInventoryAckPayload {
                revision: inventory.revision,
                accepted,
            },
        )
        .await;
        if accepted {
            spawn_wakeup(self.state);
        }
        true
    }

    async fn cancelled(mut self, payload: serde_json::Value, now: i64) -> bool {
        let Some(cancelled) = decode::<TaskCancelledPayload>(payload) else {
            return true;
        };
        let record = self
            .state
            .store
            .record_cancellation_report(self.worker_id.clone(), self.epoch, cancelled.clone(), now)
            .await;
        let (recorded, event) = record.unwrap_or((false, None));
        self.state.publish_event(event);
        self.reply(
            WorkerWireKind::TaskCancelledAck,
            TaskCancelledAckPayload {
                delivery_id: cancelled.delivery_id,
                recorded,
            },
        )
        .await;
        true
    }

    async fn resync(mut self, payload: serde_json::Value, now: i64) -> bool {
        let Some(resync) = decode::<WorkerResyncPayload>(payload) else {
            return true;
        };
        let rebind = self
            .state
            .store
            .rebind_deliveries_on_resync(
                self.worker_id.clone(),
                self.epoch,
                resync.active_delivery_ids,
                now,
            )
            .await;
        let needs_cancel = rebind.unwrap_or_default();
        self.replay_cancels(needs_cancel).await;
        spawn_wakeup(self.state);
        true
    }

    /// Replay `task.cancel` for Deliveries whose cancellation is still unacknowledged.
    async fn replay_cancels(&mut self, delivery_ids: Vec<DeliveryId>) {
        for delivery_id in delivery_ids {
            let Ok(delivery) = self.state.store.get_delivery(delivery_id).await else {
                continue;
            };
            let payload = TaskCancelPayload {
                task_id: delivery.task_id,
                delivery_id: delivery.id,
                reason: delivery.cancel_reason,
            };
            self.reply(WorkerWireKind::TaskCancel, payload).await;
        }
    }
}

fn decode<T: DeserializeOwned>(payload: serde_json::Value) -> Option<T> {
    serde_json::from_value(payload).ok()
}

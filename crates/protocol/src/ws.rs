//! Worker WSS frame envelope and typed payloads for the 14 closed V1 kinds.

use serde::{Deserialize, Serialize};

use crate::delivery::{CancelState, RejectionCode};
use crate::id::{DeliveryId, ExecutorId, SlotId, TaskId, WorkerId};
use crate::inventory::WorkerInventory;
use crate::task::TaskDefinition;

/// Maximum accepted WSS frame size (512 KiB).
pub const MAX_WS_FRAME_BYTES: usize = 512 * 1024;

/// Closed set of V1 Worker WSS frame kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkerWireKind {
    #[serde(rename = "worker.hello")]
    WorkerHello,
    #[serde(rename = "hub.welcome")]
    HubWelcome,
    #[serde(rename = "worker.heartbeat")]
    WorkerHeartbeat,
    #[serde(rename = "hub.heartbeat_ack")]
    HubHeartbeatAck,
    #[serde(rename = "worker.inventory")]
    WorkerInventory,
    #[serde(rename = "worker.inventory_ack")]
    WorkerInventoryAck,
    #[serde(rename = "task.offer")]
    TaskOffer,
    #[serde(rename = "task.rejected")]
    TaskRejected,
    #[serde(rename = "task.rejected_ack")]
    TaskRejectedAck,
    #[serde(rename = "task.cancel")]
    TaskCancel,
    #[serde(rename = "task.cancelled")]
    TaskCancelled,
    #[serde(rename = "task.cancelled_ack")]
    TaskCancelledAck,
    #[serde(rename = "worker.resync")]
    WorkerResync,
    #[serde(rename = "worker.bye")]
    WorkerBye,
}

/// Common WSS frame envelope wrapping one typed payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerWireEnvelope<T = serde_json::Value> {
    pub v: String,
    pub kind: WorkerWireKind,
    pub message_id: String,
    pub worker_id: WorkerId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_id: Option<SlotId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_epoch: Option<u64>,
    pub sent_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerHelloPayload {
    pub inventory: WorkerInventory,
    #[serde(default)]
    pub active_delivery_ids: Vec<DeliveryId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubWelcomePayload {
    pub hub_id: String,
    pub worker_id: WorkerId,
    pub connection_id: String,
    pub connection_epoch: u64,
    pub heartbeat_interval_ms: u64,
    pub lease_ttl_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerHeartbeatPayload {
    #[serde(default)]
    pub active_delivery_ids: Vec<DeliveryId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubHeartbeatAckPayload {
    pub server_time_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerInventoryAckPayload {
    pub revision: u64,
    pub accepted: bool,
}

/// `task.offer`: one committed Delivery with its immutable definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskOfferPayload {
    pub task_id: TaskId,
    pub delivery_id: DeliveryId,
    pub slot_id: SlotId,
    pub executor_id: Option<ExecutorId>,
    pub task: TaskDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRejectedPayload {
    pub task_id: TaskId,
    pub delivery_id: DeliveryId,
    pub code: RejectionCode,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRejectedAckPayload {
    pub delivery_id: DeliveryId,
    pub recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCancelPayload {
    pub task_id: TaskId,
    pub delivery_id: DeliveryId,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCancelledPayload {
    pub task_id: TaskId,
    pub delivery_id: DeliveryId,
    pub state: CancelState,
    pub reason: Option<String>,
}

/// `task.cancelled_ack`: idempotent Hub acknowledgement by `(deliveryId, epoch)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCancelledAckPayload {
    pub delivery_id: DeliveryId,
    pub recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerResyncPayload {
    #[serde(default)]
    pub active_delivery_ids: Vec<DeliveryId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerByePayload {
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [(WorkerWireKind, &str); 14] = [
        (WorkerWireKind::WorkerHello, "worker.hello"),
        (WorkerWireKind::HubWelcome, "hub.welcome"),
        (WorkerWireKind::WorkerHeartbeat, "worker.heartbeat"),
        (WorkerWireKind::HubHeartbeatAck, "hub.heartbeat_ack"),
        (WorkerWireKind::WorkerInventory, "worker.inventory"),
        (WorkerWireKind::WorkerInventoryAck, "worker.inventory_ack"),
        (WorkerWireKind::TaskOffer, "task.offer"),
        (WorkerWireKind::TaskRejected, "task.rejected"),
        (WorkerWireKind::TaskRejectedAck, "task.rejected_ack"),
        (WorkerWireKind::TaskCancel, "task.cancel"),
        (WorkerWireKind::TaskCancelled, "task.cancelled"),
        (WorkerWireKind::TaskCancelledAck, "task.cancelled_ack"),
        (WorkerWireKind::WorkerResync, "worker.resync"),
        (WorkerWireKind::WorkerBye, "worker.bye"),
    ];

    #[test]
    fn kind_set_is_closed_and_wire_named() {
        for (kind, wire) in KINDS {
            assert_eq!(serde_json::to_value(kind).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<WorkerWireKind>(serde_json::json!(wire)).unwrap(),
                kind
            );
        }
        for closed in [
            "task.finished",
            "worker.reconciliation_query",
            "hub.reconciliation",
        ] {
            assert!(serde_json::from_str::<WorkerWireKind>(&format!("\"{closed}\"")).is_err());
        }
    }

    #[test]
    fn envelope_uses_camel_case_and_skips_absent_options() {
        let frame = WorkerWireEnvelope {
            v: crate::WORKER_PROTOCOL_VERSION.to_string(),
            kind: WorkerWireKind::WorkerBye,
            message_id: "msg_1".into(),
            worker_id: WorkerId::new("wkr_000000000000"),
            slot_id: None,
            connection_id: None,
            connection_epoch: None,
            sent_at_ms: 1_764_070_000_000,
            reply_to: None,
            payload: WorkerByePayload { reason: None },
        };
        let value = serde_json::to_value(&frame).unwrap();
        assert_eq!(value["v"], crate::WORKER_PROTOCOL_VERSION);
        assert_eq!(value["messageId"], "msg_1");
        assert_eq!(value["workerId"], "wkr_000000000000");
        assert_eq!(value["sentAtMs"], 1_764_070_000_000_i64);
        assert!(value.get("slotId").is_none());
        assert!(value.get("replyTo").is_none());
        assert!(value["payload"]["reason"].is_null());
        let decoded: WorkerWireEnvelope<WorkerByePayload> = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn payloads_round_trip_and_reject_unknown_fields() {
        let welcome = HubWelcomePayload {
            hub_id: "hub_1".into(),
            worker_id: WorkerId::new("wkr_000000000000"),
            connection_id: "conn_1".into(),
            connection_epoch: 2,
            heartbeat_interval_ms: 5_000,
            lease_ttl_ms: 15_000,
        };
        let value = serde_json::to_value(&welcome).unwrap();
        assert_eq!(value["connectionEpoch"], 2);
        assert_eq!(value["leaseTtlMs"], 15_000);
        assert_eq!(
            serde_json::from_value::<HubWelcomePayload>(value).unwrap(),
            welcome
        );

        let resync: WorkerResyncPayload = serde_json::from_str("{}").unwrap();
        assert!(resync.active_delivery_ids.is_empty());
        assert!(
            serde_json::from_str::<WorkerByePayload>(r#"{"reason":"done","extra":1}"#).is_err()
        );
    }
}

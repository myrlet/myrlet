//! Delivery: one durable Hub-to-Worker handoff of a Task, with refusal and cancel states.

use crate::id::{DeliveryId, ExecutorId, SlotId, TaskId, WorkerId};
use crate::progress::DeliveryProgressView;
use serde::{Deserialize, Serialize};

/// Delivery lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Offered,
    Accepted,
    Rejected,
    Released,
    Finished,
    Cancelled,
}

/// Pre-registration refusal reason reported by a Worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCode {
    ExecutorUnavailable,
    WorkerShuttingDown,
    RecoveryIncomplete,
}

/// Harness cancellation response relayed by a Worker; never a claim of physical termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelState {
    Confirmed,
    Failed,
    Uncertain,
    Unsupported,
}

/// Current Delivery summary attached to Task reads, the Snapshot, and offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryView {
    pub id: DeliveryId,
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub slot_id: SlotId,
    pub executor_id: Option<ExecutorId>,
    /// Epoch of the WSS connection this Delivery is fenced to.
    pub connection_epoch: u64,
    pub state: DeliveryStatus,
    pub rejection_code: Option<RejectionCode>,
    pub cancel_state: Option<CancelState>,
    pub cancel_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Latest accepted Progress for this Delivery, if any.
    pub progress: Option<DeliveryProgressView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure_view() -> DeliveryView {
        DeliveryView {
            id: DeliveryId::new("delivery_01"),
            task_id: TaskId::new("task_01"),
            worker_id: WorkerId::new("wkr_0123456789AB"),
            slot_id: SlotId::new("slot_1"),
            executor_id: None,
            connection_epoch: 7,
            state: DeliveryStatus::Rejected,
            rejection_code: Some(RejectionCode::RecoveryIncomplete),
            cancel_state: None,
            cancel_reason: None,
            created_at_ms: 10,
            updated_at_ms: 11,
            progress: None,
        }
    }

    #[test]
    fn view_serializes_camel_case_with_snake_case_enums() {
        let json = serde_json::to_value(failure_view()).unwrap();
        assert_eq!(json["taskId"], "task_01");
        assert_eq!(json["connectionEpoch"], 7);
        assert_eq!(json["rejectionCode"], "recovery_incomplete");
        assert_eq!(json["state"], "rejected");
        assert!(json["executorId"].is_null());
    }

    #[test]
    fn cancel_states_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&CancelState::Unsupported).unwrap(),
            "\"unsupported\""
        );
    }
}

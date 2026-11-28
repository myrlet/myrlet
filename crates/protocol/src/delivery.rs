//! Delivery: one committed Hub-to-Worker handoff of a Task, created by an explicit claim.

use crate::id::{DeliveryId, ExecutorId, SlotId, TaskId, WorkerId};
use crate::progress::DeliveryProgressView;
use serde::{Deserialize, Serialize};

/// Delivery lifecycle states.
///
/// A claim commits the Delivery directly: there is no offer to accept, refuse, or
/// release, so `accepted` is the only live state before handling ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Accepted,
    Finished,
    Cancelled,
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

/// Current Delivery summary attached to Task reads, the Snapshot, and claim replies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryView {
    pub id: DeliveryId,
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub slot_id: SlotId,
    pub executor_id: Option<ExecutorId>,
    /// Epoch of the WSS connection that committed this Delivery.
    pub connection_epoch: u64,
    pub state: DeliveryStatus,
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

    fn view() -> DeliveryView {
        DeliveryView {
            id: DeliveryId::new("delivery_01"),
            task_id: TaskId::new("task_01"),
            worker_id: WorkerId::new("wkr_0123456789AB"),
            slot_id: SlotId::new("slot_1"),
            executor_id: None,
            connection_epoch: 7,
            state: DeliveryStatus::Accepted,
            cancel_state: None,
            cancel_reason: None,
            created_at_ms: 10,
            updated_at_ms: 11,
            progress: None,
        }
    }

    #[test]
    fn view_serializes_camel_case_with_snake_case_enums() {
        let json = serde_json::to_value(view()).unwrap();
        assert_eq!(json["taskId"], "task_01");
        assert_eq!(json["connectionEpoch"], 7);
        assert_eq!(json["state"], "accepted");
        assert!(json["executorId"].is_null());
        assert!(json["cancelState"].is_null());
    }

    #[test]
    fn delivery_states_are_the_three_committed_states() {
        assert_eq!(
            serde_json::to_string(&DeliveryStatus::Accepted).unwrap(),
            "\"accepted\""
        );
        assert_eq!(
            serde_json::to_string(&DeliveryStatus::Finished).unwrap(),
            "\"finished\""
        );
        assert_eq!(
            serde_json::to_string(&DeliveryStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
        for dead in ["offered", "rejected", "released"] {
            assert!(serde_json::from_str::<DeliveryStatus>(&format!("\"{dead}\"")).is_err());
        }
    }

    #[test]
    fn cancel_states_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&CancelState::Unsupported).unwrap(),
            "\"unsupported\""
        );
    }
}

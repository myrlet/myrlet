//! Hub HTTP command/query DTOs (`camelCase` on the wire).

use serde::{Deserialize, Serialize};

use crate::delivery::DeliveryView;
use crate::id::{ExecutorId, SlotId, TaskId, WorkerId};
use crate::snapshot::SnapshotWorker;
use crate::task::{TaskRecord, TaskStatus};

/// Stable error body; never leaks internal SQLite causes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

/// Process liveness probe body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// `POST /v1/enrollment-codes` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateEnrollmentCodeRequest {
    pub ttl_seconds: Option<u64>,
    pub suggested_worker_id: Option<WorkerId>,
}

/// `POST /v1/enrollment-codes` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEnrollmentCodeResponse {
    pub enrollment_code: String,
    pub expires_at_ms: i64,
    pub suggested_worker_id: Option<WorkerId>,
}

/// `POST /v1/pair` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairWorkerRequest {
    pub enrollment_code: String,
    pub worker_id: Option<WorkerId>,
}

/// `POST /v1/pair` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairWorkerResponse {
    pub hub_id: String,
    pub worker_id: WorkerId,
    pub credential: String,
}

/// `POST /v1/workers/{id}/rotate` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateWorkerCredentialResponse {
    pub hub_id: String,
    pub worker_id: WorkerId,
    pub credential: String,
}

/// How a dispatch command picks a Worker/Slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum DispatchSelection {
    #[serde(rename_all = "camelCase")]
    Targeted {
        worker_id: WorkerId,
        slot_id: SlotId,
        executor_id: Option<ExecutorId>,
    },
    #[serde(rename_all = "camelCase")]
    Match {
        #[serde(default)]
        fallback_allowed: bool,
    },
}

/// `POST /v1/tasks/{id}/dispatch` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DispatchTaskRequest {
    pub expected_task_version: u64,
    pub selection: DispatchSelection,
    #[serde(default)]
    pub queue: bool,
}

/// `POST /v1/tasks/{id}/dispatch` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchTaskResponse {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub version: u64,
    pub delivery: Option<DeliveryView>,
}

/// `POST /v1/tasks/{id}/cancel` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelTaskRequest {
    pub expected_task_version: u64,
    pub reason: Option<String>,
}

/// `POST /v1/tasks/{id}/finish` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinishTaskRequest {
    pub expected_task_version: u64,
}

/// Direction of a bounded Task tree read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTreeDirection {
    Ancestors,
    Descendants,
}

/// `GET /v1/tasks/{id}/tree` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTreeResponse {
    pub root_task_id: TaskId,
    pub tasks: Vec<TaskRecord>,
    pub truncated: bool,
}

/// `GET /v1/workers` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerListResponse {
    pub workers: Vec<SnapshotWorker>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_selection_is_tagged_and_camel_case() {
        let match_mode = DispatchSelection::Match {
            fallback_allowed: true,
        };
        assert_eq!(
            serde_json::to_value(&match_mode).unwrap(),
            serde_json::json!({ "mode": "match", "fallbackAllowed": true })
        );

        let targeted = DispatchSelection::Targeted {
            worker_id: WorkerId::new("wkr_000000000000"),
            slot_id: SlotId::new("slot-a"),
            executor_id: None,
        };
        let value = serde_json::to_value(&targeted).unwrap();
        assert_eq!(value["mode"], "targeted");
        assert_eq!(value["workerId"], "wkr_000000000000");
        assert_eq!(value["slotId"], "slot-a");
        assert_eq!(
            serde_json::from_value::<DispatchSelection>(value).unwrap(),
            targeted
        );
    }

    #[test]
    fn dispatch_request_defaults_and_rejects_unknown_fields() {
        let request: DispatchTaskRequest =
            serde_json::from_str(r#"{"expectedTaskVersion":4,"selection":{"mode":"match"}}"#)
                .unwrap();
        assert_eq!(request.expected_task_version, 4);
        assert!(!request.queue);
        assert_eq!(
            request.selection,
            DispatchSelection::Match {
                fallback_allowed: false
            }
        );

        let unknown = r#"{"expectedTaskVersion":4,"selection":{"mode":"match"},"bogus":1}"#;
        assert!(serde_json::from_str::<DispatchTaskRequest>(unknown).is_err());
        assert!(serde_json::from_str::<DispatchSelection>(r#"{"mode":"fanout"}"#).is_err());
    }

    #[test]
    fn enrollment_request_and_tree_response_use_wire_names() {
        let request: CreateEnrollmentCodeRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(request.ttl_seconds, None);
        assert_eq!(request.suggested_worker_id, None);

        let tree = TaskTreeResponse {
            root_task_id: TaskId::new("task_root"),
            tasks: Vec::new(),
            truncated: false,
        };
        let value = serde_json::to_value(&tree).unwrap();
        assert_eq!(value["rootTaskId"], "task_root");
        assert_eq!(value["truncated"], false);
        assert_eq!(
            serde_json::to_value(TaskTreeDirection::Ancestors).unwrap(),
            "ancestors"
        );
    }
}

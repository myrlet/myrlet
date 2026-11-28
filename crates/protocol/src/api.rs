//! Hub HTTP command/query DTOs (`camelCase` on the wire).

use serde::{Deserialize, Serialize};

use crate::delivery::DeliveryView;
use crate::id::{DeliveryId, ExecutorId, SlotId, TaskId, WorkerId};
use crate::snapshot::SnapshotWorker;
use crate::task::TaskRecord;

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

/// `POST /v1/tasks` response: the authorized Task and whether it became `queued`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTaskResponse {
    pub task: TaskRecord,
    /// `true` when every prerequisite was already `finished` and the Task is `queued`.
    pub eligible: bool,
}

/// Fields an Owner may still change while a Task has no Delivery.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingTaskUpdate {
    #[serde(default)]
    pub dependencies: Option<Vec<TaskId>>,
    #[serde(default)]
    pub routing: Option<crate::task::TaskRouting>,
}

/// `POST /v1/tasks/{id}` request: version-fenced edits to an unclaimed Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateTaskRequest {
    pub expected_task_version: u64,
    #[serde(flatten)]
    pub update: PendingTaskUpdate,
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

/// `GET /v1/tasks` response: bounded Task summaries plus the Snapshot event cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListResponse {
    pub tasks: Vec<crate::snapshot::SnapshotTaskSummary>,
    pub total_tasks: u64,
    pub truncated: bool,
    pub event_cursor: u64,
}

/// `GET /v1/workers` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerListResponse {
    pub workers: Vec<SnapshotWorker>,
}

/// `POST /v1/workers/claim` request: claim one `queued` Task for one local Slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimTaskRequest {
    pub task_id: TaskId,
    pub slot_id: SlotId,
    /// The `connectionEpoch` of the WSS session making the claim.
    pub connection_epoch: u64,
    pub executor_id: Option<ExecutorId>,
}

/// `POST /v1/workers/claim` response: the one committed Delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimTaskResponse {
    pub task: TaskRecord,
    pub delivery: DeliveryView,
}

/// Whether a Task may be claimed right now, and why not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEligibility {
    pub task_id: TaskId,
    /// `queued` Tasks with finished prerequisites and no Delivery are claimable.
    pub claimable: bool,
    /// Blocking prerequisite IDs with their status; empty once every one is `finished`.
    pub blocked_by: Vec<crate::task::BlockingDependency>,
}

/// `GET /v1/workers/eligibility` response for one Worker's advertised Slots.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EligibilityResponse {
    pub tasks: Vec<TaskEligibility>,
}

/// `GET /v1/workers/{id}` detail: applied inventory plus the reported snapshot under review
/// and the next claimable Task IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerDetail {
    pub worker: SnapshotWorker,
    /// The latest accepted scheduling report; the applied snapshot lives on `worker`.
    pub reported_config: Option<crate::trust::WorkerSchedulingConfig>,
    /// Claimable Task IDs that this Worker's applied Slots could run, oldest first.
    #[serde(default)]
    pub claimable_task_ids: Vec<TaskId>,
}

/// Effective status of the Worker-local result compare performed by `result status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultComparison {
    /// Local and Hub bytes are identical.
    Equal,
    /// Both sides have content, and it differs.
    Different,
    /// The Worker has content and the Hub has none.
    MissingRemotely,
    /// The Hub has content and the Worker has none.
    MissingLocally,
    /// Neither side has any content.
    Absent,
    /// The Hub could not be reached; nothing was compared.
    Unknown,
}

/// `GET /v1/workers/deliveries/{id}/result` response used to compare one Worker copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryResultView {
    pub delivery_id: DeliveryId,
    pub task_id: TaskId,
    pub text: Option<String>,
    pub recorded_at_ms: Option<i64>,
    pub result_sha256: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{TaskDefinition, TaskStatus};

    #[test]
    fn update_request_flattens_the_pending_fields() {
        let raw = r#"{"expectedTaskVersion":3,"dependencies":["task_a"]}"#;
        let request: UpdateTaskRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(request.expected_task_version, 3);
        assert_eq!(
            request.update.dependencies,
            Some(vec![TaskId::new("task_a")])
        );
        assert!(request.update.routing.is_none());
        assert!(serde_json::from_str::<UpdateTaskRequest>(
            r#"{"expectedTaskVersion":3,"title":"no"}"#
        )
        .is_err());
    }

    #[test]
    fn claim_request_and_eligibility_use_wire_names() {
        let raw = r#"{"taskId":"task_1","slotId":"slot-a","connectionEpoch":7}"#;
        let request: ClaimTaskRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(request.connection_epoch, 7);
        let value = serde_json::to_value(EligibilityResponse {
            tasks: vec![TaskEligibility {
                task_id: TaskId::new("task_1"),
                claimable: false,
                blocked_by: Vec::new(),
            }],
        })
        .unwrap();
        assert_eq!(value["tasks"][0]["taskId"], "task_1");
        assert_eq!(value["tasks"][0]["claimable"], false);
        assert_eq!(
            serde_json::to_value(ResultComparison::MissingRemotely).unwrap(),
            "missing_remotely"
        );
    }

    #[test]
    fn submit_response_nests_the_task() {
        let record = TaskRecord {
            id: TaskId::new("task_01"),
            root_task_id: TaskId::new("task_01"),
            depth: 1,
            status: TaskStatus::Queued,
            version: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
            finished_at_ms: None,
            task: TaskDefinition {
                client_ref: None,
                parent_task_id: None,
                source_task_id: None,
                dependencies: Vec::new(),
                category: "general".into(),
                tags: Vec::new(),
                title: "t".into(),
                prompt: "p".into(),
                input: None,
                expected_output: None,
                external_references: Vec::new(),
                routing: Default::default(),
                execution: Default::default(),
                metadata: Default::default(),
            },
            current_delivery: None,
            blocked_by: Vec::new(),
            result_markdown: None,
            result_recorded_at_ms: None,
            subtree: Default::default(),
        };
        let value = serde_json::to_value(SubmitTaskResponse {
            task: record,
            eligible: true,
        })
        .unwrap();
        assert_eq!(value["task"]["status"], "queued");
        assert_eq!(value["eligible"], true);
    }
}

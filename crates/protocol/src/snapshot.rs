//! Global Snapshot and Hub event stream wire DTOs (`GET /v1/snapshot`, SSE).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::delivery::DeliveryView;
use crate::id::{TaskId, WorkerId};
use crate::inventory::HostFacts;
use crate::task::{BlockingDependency, SubtreeCounts, TaskStatus};
use crate::trust::{PermissionTier, WorkerApprovalStatus, WorkerSchedulingConfig};

/// One Worker's connectivity, approval, permission tier, and applied scheduling inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotWorker {
    pub worker_id: WorkerId,
    pub online: bool,
    pub revoked: bool,
    pub approval: WorkerApprovalStatus,
    /// Server-controlled tier of this Worker principal; never supplied by the client.
    pub permission_tier: PermissionTier,
    pub connection_epoch: u64,
    pub last_seen_at_ms: Option<i64>,
    pub active_deliveries: u64,
    /// Host facts from the latest accepted report; ordinary telemetry, never a gate.
    pub host: Option<HostFacts>,
    /// Hub-assigned revision of the latest accepted report, or `null` before one arrives.
    pub reported_revision: Option<u64>,
    /// Hub-assigned revision the Hub uses for Slot matching, or `null` before Apply.
    pub applied_revision: Option<u64>,
    /// The scheduling snapshot the Hub uses for Slot matching.
    pub applied_config: Option<WorkerSchedulingConfig>,
}

/// Bounded read model of one Task and its current Delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotTaskSummary {
    pub id: TaskId,
    pub parent_task_id: Option<TaskId>,
    pub root_task_id: TaskId,
    pub depth: u8,
    pub source_task_id: Option<TaskId>,
    #[serde(default)]
    pub dependencies: Vec<TaskId>,
    /// Dependencies that are not `finished`, so the reason a Task is still `held` is visible.
    #[serde(default)]
    pub blocked_by: Vec<BlockingDependency>,
    pub category: String,
    pub title: String,
    pub status: TaskStatus,
    pub version: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub current_delivery: Option<DeliveryView>,
    pub subtree: SubtreeCounts,
}

/// Consistent cluster-wide read model returned by `GET /v1/snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSnapshot {
    pub schema_version: String,
    pub generated_at_ms: i64,
    pub event_cursor: u64,
    pub tasks_truncated: bool,
    pub total_tasks: u64,
    pub workers: Vec<SnapshotWorker>,
    pub tasks: Vec<SnapshotTaskSummary>,
}

/// One invalidation hint replayed on the Hub SSE event stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubEventEnvelope {
    pub seq: u64,
    pub kind: String,
    pub created_at_ms: i64,
    pub payload: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::DeploymentMode;

    #[test]
    fn snapshot_serializes_camel_case_and_keeps_null_capacity() {
        let snapshot = GlobalSnapshot {
            schema_version: crate::SNAPSHOT_SCHEMA_VERSION.to_string(),
            generated_at_ms: 1_764_070_000_500,
            event_cursor: 41,
            tasks_truncated: false,
            total_tasks: 0,
            workers: vec![SnapshotWorker {
                worker_id: WorkerId::new("wkr_000000000000"),
                online: true,
                revoked: false,
                approval: WorkerApprovalStatus::Approved,
                permission_tier: PermissionTier::Worker,
                connection_epoch: 7,
                last_seen_at_ms: None,
                active_deliveries: 0,
                host: Some(HostFacts {
                    os: "linux".into(),
                    arch: "x86_64".into(),
                    deployment_mode: DeploymentMode::Native,
                }),
                reported_revision: Some(1),
                applied_revision: Some(1),
                applied_config: Some(WorkerSchedulingConfig { slots: Vec::new() }),
            }],
            tasks: Vec::new(),
        };

        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(value["schemaVersion"], crate::SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(value["eventCursor"], 41);
        assert_eq!(value["workers"][0]["workerId"], "wkr_000000000000");
        assert_eq!(value["workers"][0]["connectionEpoch"], 7);
        assert_eq!(value["workers"][0]["approval"], "approved");
        assert_eq!(value["workers"][0]["permissionTier"], "worker");
        assert_eq!(value["workers"][0]["reportedRevision"], 1);
        assert_eq!(value["workers"][0]["appliedRevision"], 1);
        assert!(value["workers"][0]["lastSeenAtMs"].is_null());
        assert_eq!(value["workers"][0]["host"]["deploymentMode"], "native");

        let decoded: GlobalSnapshot = serde_json::from_value(value).unwrap();
        assert!(decoded.tasks.is_empty());
        assert!(decoded.workers[0].applied_config.is_some());
    }

    #[test]
    fn event_envelope_carries_kind_payload_and_camel_case_keys() {
        let event = HubEventEnvelope {
            seq: 9,
            kind: "task.status_changed".into(),
            created_at_ms: 1_764_070_000_900,
            payload: serde_json::json!({ "taskId": "task_01" }),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["seq"], 9);
        assert_eq!(value["createdAtMs"], 1_764_070_000_900_i64);
        assert_eq!(value["payload"]["taskId"], "task_01");
        assert_eq!(
            serde_json::from_value::<HubEventEnvelope>(value).unwrap(),
            event
        );
    }
}

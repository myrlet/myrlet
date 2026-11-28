//! Async `HubStore` facade for Task, bundle, claim, read, and event operations.

use std::collections::HashMap;

use myrlet_protocol::{
    CancelTaskRequest, ClaimTaskRequest, ClaimTaskResponse, FinishTaskRequest, GlobalSnapshot,
    HubEventEnvelope, TaskBundleImportRequest, TaskDefinition, TaskId, TaskListResponse,
    TaskReadResponse, TaskRecord, TaskTreeDirection, TaskTreeResponse, UpdateTaskRequest, WorkerId,
};

use crate::{
    bundle, claim, commands, events, reads, scoped, tasks, BundleOutcome, CancellationOutcome,
    HubStore, StoreError, TaskMutation,
};

impl HubStore {
    pub async fn latest_seq(&self) -> Result<u64, StoreError> {
        self.db.call(|conn| events::latest_seq(conn)).await
    }

    pub async fn list_events_after(
        &self,
        after_seq: u64,
        limit: usize,
        retention_window: usize,
    ) -> Result<Vec<HubEventEnvelope>, StoreError> {
        self.db
            .call(move |conn| events::list_events_after(conn, after_seq, limit, retention_window))
            .await
    }

    pub async fn create_task(
        &self,
        idempotency_key: String,
        task: TaskDefinition,
        now_ms: i64,
    ) -> Result<TaskMutation, StoreError> {
        self.db
            .call(move |conn| commands::create_task(conn, &idempotency_key, &task, now_ms))
            .await
    }

    pub async fn update_task(
        &self,
        idempotency_key: String,
        task_id: TaskId,
        request: UpdateTaskRequest,
        now_ms: i64,
    ) -> Result<TaskMutation, StoreError> {
        self.db
            .call(move |conn| {
                commands::update_task(conn, &idempotency_key, &task_id, &request, now_ms)
            })
            .await
    }

    pub async fn import_bundle(
        &self,
        idempotency_key: String,
        request: TaskBundleImportRequest,
        now_ms: i64,
    ) -> Result<BundleOutcome, StoreError> {
        self.db
            .call(move |conn| bundle::import_bundle(conn, &idempotency_key, &request, now_ms))
            .await
    }

    pub async fn get_task(&self, task_id: TaskId) -> Result<TaskRecord, StoreError> {
        self.db
            .call(move |conn| reads::get_task(conn, &task_id))
            .await
    }

    /// Worker-tier Task detail: the assigned Task in full or a related summary, plus the
    /// one-hop related summaries. `None` means outside the Worker's visibility boundary.
    pub async fn worker_task_read(
        &self,
        worker_id: WorkerId,
        task_id: TaskId,
    ) -> Result<Option<TaskReadResponse>, StoreError> {
        self.db
            .call(move |conn| scoped::task_read_for_worker(conn, &worker_id, &task_id))
            .await
    }

    /// Worker-tier Task tree anchored to the Worker's own Delivery-bound Task.
    pub async fn worker_task_tree(
        &self,
        worker_id: WorkerId,
        task_id: TaskId,
    ) -> Result<Option<TaskTreeResponse>, StoreError> {
        self.db
            .call(move |conn| scoped::task_tree_for_worker(conn, &worker_id, &task_id))
            .await
    }

    /// Worker-tier eligibility for the Worker's own Task or a directly related Task.
    pub async fn worker_task_eligibility(
        &self,
        worker_id: WorkerId,
        task_id: TaskId,
    ) -> Result<Option<myrlet_protocol::TaskEligibility>, StoreError> {
        self.db
            .call(move |conn| scoped::eligibility_for_worker(conn, &worker_id, &task_id))
            .await
    }

    pub async fn list_tasks(&self, limit: usize) -> Result<TaskListResponse, StoreError> {
        self.db
            .call(move |conn| reads::list_tasks(conn, limit))
            .await
    }

    pub async fn get_task_tree(
        &self,
        task_id: TaskId,
        direction: TaskTreeDirection,
        max_depth: u8,
        limit: usize,
    ) -> Result<TaskTreeResponse, StoreError> {
        self.db
            .call(move |conn| reads::get_task_tree(conn, &task_id, direction, max_depth, limit))
            .await
    }

    pub async fn list_snapshot(
        &self,
        online_map: HashMap<WorkerId, u64>,
        max_tasks: usize,
        now_ms: i64,
    ) -> Result<GlobalSnapshot, StoreError> {
        self.db
            .call(move |conn| reads::list_snapshot(conn, &online_map, max_tasks, now_ms))
            .await
    }

    pub async fn cancel_task(
        &self,
        idempotency_key: String,
        task_id: TaskId,
        request: CancelTaskRequest,
        now_ms: i64,
    ) -> Result<CancellationOutcome, StoreError> {
        self.db
            .call(move |conn| {
                commands::cancel_task(conn, &idempotency_key, &task_id, &request, now_ms)
            })
            .await
    }

    pub async fn finish_coordination_task(
        &self,
        idempotency_key: String,
        task_id: TaskId,
        request: FinishTaskRequest,
        now_ms: i64,
    ) -> Result<TaskMutation, StoreError> {
        self.db
            .call(move |conn| {
                commands::finish_coordination_task(
                    conn,
                    &idempotency_key,
                    &task_id,
                    &request,
                    now_ms,
                )
            })
            .await
    }

    pub async fn claim_task(
        &self,
        worker_id: WorkerId,
        request: ClaimTaskRequest,
        now_ms: i64,
    ) -> Result<(ClaimTaskResponse, Option<HubEventEnvelope>), StoreError> {
        self.db
            .call(move |conn| claim::claim_task(conn, &worker_id, &request, now_ms))
            .await
    }

    /// Claimability of one Task, used for diagnostic reads.
    pub async fn task_eligibility(
        &self,
        task_id: TaskId,
    ) -> Result<myrlet_protocol::TaskEligibility, StoreError> {
        self.db
            .call(move |conn| reads::eligibility(conn, &task_id))
            .await
    }

    /// Oldest claimable Task IDs for one Worker's applied Slots.
    pub async fn claimable_for_worker(
        &self,
        worker_id: WorkerId,
        limit: usize,
        online_map: HashMap<WorkerId, u64>,
    ) -> Result<Vec<TaskId>, StoreError> {
        self.db
            .call(move |conn| {
                let worker = crate::inventory::get_worker_snapshot(conn, &worker_id, &online_map)?;
                if !worker.is_schedulable() {
                    return Ok(Vec::new());
                }
                reads::claimable_for_worker(conn, worker.applied_config.as_ref(), limit)
            })
            .await
    }

    pub async fn list_queued_tasks(
        &self,
        limit: usize,
    ) -> Result<Vec<(TaskId, TaskDefinition)>, StoreError> {
        self.db
            .call(move |conn| reads::list_queued_tasks(conn, limit))
            .await
    }

    pub async fn are_dependencies_satisfied(&self, task_id: TaskId) -> Result<bool, StoreError> {
        self.db
            .call(move |conn| tasks::are_dependencies_satisfied(conn, &task_id))
            .await
    }
}

//! Async `HubStore` facade for Task, bundle, dispatch, read, and event operations.

use std::collections::HashMap;

use myrlet_protocol::{
    CancelTaskRequest, DeliveryId, DispatchTaskRequest, FinishTaskRequest, GlobalSnapshot,
    HubEventEnvelope, TaskBundleImportRequest, TaskDefinition, TaskId, TaskRecord,
    TaskTreeDirection, TaskTreeResponse, WorkerId,
};

use crate::{
    bundle, commands, dispatch, events, reads, tasks, BundleOutcome, CancellationOutcome,
    DeliveryTarget, DispatchOutcome, HubStore, StoreError, TaskMutation,
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

    pub async fn are_dependencies_satisfied(&self, task_id: TaskId) -> Result<bool, StoreError> {
        self.db
            .call(move |conn| tasks::are_dependencies_satisfied(conn, &task_id))
            .await
    }

    pub async fn list_queued_tasks(
        &self,
        limit: usize,
    ) -> Result<Vec<(TaskId, u64, TaskDefinition, DispatchTaskRequest)>, StoreError> {
        self.db
            .call(move |conn| reads::list_queued_tasks(conn, limit))
            .await
    }

    pub async fn enqueue_task(
        &self,
        idempotency_key: String,
        task_id: TaskId,
        request: DispatchTaskRequest,
        now_ms: i64,
    ) -> Result<DispatchOutcome, StoreError> {
        self.db
            .call(move |conn| {
                dispatch::enqueue_task(conn, &idempotency_key, &task_id, &request, now_ms)
            })
            .await
    }

    pub async fn create_delivery(
        &self,
        idempotency_key: Option<String>,
        task_id: TaskId,
        expected_version: u64,
        target: DeliveryTarget,
        now_ms: i64,
    ) -> Result<DispatchOutcome, StoreError> {
        self.db
            .call(move |conn| {
                dispatch::create_delivery(
                    conn,
                    idempotency_key.as_deref(),
                    &task_id,
                    expected_version,
                    &target,
                    now_ms,
                )
            })
            .await
    }

    pub async fn release_delivery(
        &self,
        delivery_id: DeliveryId,
        requeue: bool,
        now_ms: i64,
    ) -> Result<Option<HubEventEnvelope>, StoreError> {
        self.db
            .call(move |conn| dispatch::release_delivery(conn, &delivery_id, requeue, now_ms))
            .await
    }
}

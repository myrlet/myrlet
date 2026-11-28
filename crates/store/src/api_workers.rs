//! Async `HubStore` facade for Worker enrollment, inventory, and Delivery reporting.

use std::collections::HashMap;

use myrlet_protocol::{
    CreateEnrollmentCodeResponse, DeliveryId, DeliveryView, FinishReceipt, FinishReportRequest,
    HubEventEnvelope, PairWorkerRequest, PairWorkerResponse, ProgressReceipt,
    ProgressReportRequest, ResultReceipt, ResultReportRequest, RotateWorkerCredentialResponse,
    SecurityTrustSettings, SnapshotWorker, TaskCancelledPayload, UpdateSecurityTrustRequest,
    WorkerDetail, WorkerId, WorkerInventory,
};

use crate::{
    deliveries, inventory, progress, reports, result, settings, workers, HubStore, StoreError,
};

impl HubStore {
    /// The two Hub-wide Security & Trust policies.
    pub async fn load_settings(&self) -> Result<SecurityTrustSettings, StoreError> {
        self.db.call(|conn| settings::load_settings(conn)).await
    }

    /// Merge a policy patch and return the stored pair.
    pub async fn update_settings(
        &self,
        request: UpdateSecurityTrustRequest,
        now_ms: i64,
    ) -> Result<(SecurityTrustSettings, HubEventEnvelope), StoreError> {
        self.db
            .call(move |conn| settings::update_settings(conn, &request, now_ms))
            .await
    }

    /// Approve a pending Worker; repeating approval is harmless.
    pub async fn approve_worker(
        &self,
        worker_id: WorkerId,
        now_ms: i64,
    ) -> Result<HubEventEnvelope, StoreError> {
        self.db
            .call(move |conn| workers::approve_worker(conn, &worker_id, now_ms))
            .await
    }

    /// Apply a Worker's latest scheduling report at the revision the Admin reviewed.
    pub async fn apply_worker_config(
        &self,
        worker_id: WorkerId,
        revision: u64,
        now_ms: i64,
    ) -> Result<u64, StoreError> {
        self.db
            .call(move |conn| inventory::apply_worker_config(conn, &worker_id, revision, now_ms))
            .await
    }

    pub async fn record_cancellation_report(
        &self,
        worker_id: WorkerId,
        epoch: u64,
        payload: TaskCancelledPayload,
        now_ms: i64,
    ) -> Result<(bool, Option<HubEventEnvelope>), StoreError> {
        self.db
            .call(move |conn| {
                reports::record_cancellation_report(conn, &worker_id, epoch, &payload, now_ms)
            })
            .await
    }

    pub async fn rebind_deliveries_on_resync(
        &self,
        worker_id: WorkerId,
        new_epoch: u64,
        active_delivery_ids: Vec<DeliveryId>,
        now_ms: i64,
    ) -> Result<Vec<DeliveryId>, StoreError> {
        self.db
            .call(move |conn| {
                reports::rebind_deliveries_on_resync(
                    conn,
                    &worker_id,
                    new_epoch,
                    &active_delivery_ids,
                    now_ms,
                )
            })
            .await
    }

    pub async fn record_progress(
        &self,
        worker_id: WorkerId,
        delivery_id: DeliveryId,
        request: ProgressReportRequest,
        now_ms: i64,
    ) -> Result<(ProgressReceipt, Option<HubEventEnvelope>), StoreError> {
        self.db
            .call(move |conn| {
                progress::record_progress(conn, &worker_id, &delivery_id, &request, now_ms)
            })
            .await
    }

    pub async fn record_finish(
        &self,
        worker_id: WorkerId,
        delivery_id: DeliveryId,
        request: FinishReportRequest,
        now_ms: i64,
    ) -> Result<(FinishReceipt, Option<HubEventEnvelope>), StoreError> {
        self.db
            .call(move |conn| {
                progress::record_finish(conn, &worker_id, &delivery_id, &request, now_ms)
            })
            .await
    }

    pub async fn record_result(
        &self,
        worker_id: WorkerId,
        delivery_id: DeliveryId,
        request: ResultReportRequest,
        now_ms: i64,
    ) -> Result<(ResultReceipt, Option<HubEventEnvelope>), StoreError> {
        self.db
            .call(move |conn| {
                result::record_result(conn, &worker_id, &delivery_id, &request, now_ms)
            })
            .await
    }

    pub async fn create_enrollment_code(
        &self,
        ttl_seconds: Option<u64>,
        suggested_worker_id: Option<WorkerId>,
        now_ms: i64,
    ) -> Result<CreateEnrollmentCodeResponse, StoreError> {
        self.db
            .call(move |conn| {
                workers::create_enrollment_code(
                    conn,
                    ttl_seconds,
                    suggested_worker_id.as_ref(),
                    now_ms,
                )
            })
            .await
    }

    pub async fn redeem_pairing_code(
        &self,
        hub_id: String,
        request: PairWorkerRequest,
        now_ms: i64,
    ) -> Result<(PairWorkerResponse, HubEventEnvelope), StoreError> {
        self.db
            .call(move |conn| workers::redeem_pairing_code(conn, &hub_id, &request, now_ms))
            .await
    }

    pub async fn authenticate_worker(&self, bearer_token: String) -> Result<WorkerId, StoreError> {
        self.db
            .call(move |conn| workers::authenticate_worker(conn, &bearer_token))
            .await
    }

    pub async fn rotate_worker_credential(
        &self,
        hub_id: String,
        worker_id: WorkerId,
        now_ms: i64,
    ) -> Result<(RotateWorkerCredentialResponse, HubEventEnvelope), StoreError> {
        self.db
            .call(move |conn| workers::rotate_worker_credential(conn, &hub_id, &worker_id, now_ms))
            .await
    }

    pub async fn revoke_worker(
        &self,
        worker_id: WorkerId,
        now_ms: i64,
    ) -> Result<HubEventEnvelope, StoreError> {
        self.db
            .call(move |conn| workers::revoke_worker(conn, &worker_id, now_ms))
            .await
    }

    pub async fn open_worker_connection(
        &self,
        worker_id: WorkerId,
        inventory: WorkerInventory,
        now_ms: i64,
    ) -> Result<u64, StoreError> {
        self.db
            .call(move |conn| {
                inventory::open_worker_connection(conn, &worker_id, &inventory, now_ms)
            })
            .await
    }

    pub async fn update_worker_inventory(
        &self,
        worker_id: WorkerId,
        epoch: u64,
        inventory: WorkerInventory,
        now_ms: i64,
    ) -> Result<bool, StoreError> {
        self.db
            .call(move |conn| {
                inventory::update_worker_inventory(conn, &worker_id, epoch, &inventory, now_ms)
            })
            .await
    }

    pub async fn touch_worker_seen(
        &self,
        worker_id: WorkerId,
        epoch: u64,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        self.db
            .call(move |conn| inventory::touch_worker_seen(conn, &worker_id, epoch, now_ms))
            .await
    }

    pub async fn list_workers(
        &self,
        online_map: HashMap<WorkerId, u64>,
    ) -> Result<Vec<SnapshotWorker>, StoreError> {
        self.db
            .call(move |conn| inventory::list_workers(conn, &online_map))
            .await
    }

    pub async fn get_delivery(&self, delivery_id: DeliveryId) -> Result<DeliveryView, StoreError> {
        self.db
            .call(move |conn| {
                let row = deliveries::load_delivery(conn, &delivery_id)?;
                deliveries::delivery_view(conn, &row)
            })
            .await
    }

    pub async fn get_worker(
        &self,
        worker_id: WorkerId,
        online_map: HashMap<WorkerId, u64>,
    ) -> Result<SnapshotWorker, StoreError> {
        self.db
            .call(move |conn| inventory::get_worker(conn, &worker_id, &online_map))
            .await
    }

    pub async fn worker_detail(
        &self,
        worker_id: WorkerId,
        online_map: HashMap<WorkerId, u64>,
        claimable_limit: usize,
    ) -> Result<WorkerDetail, StoreError> {
        self.db
            .call(move |conn| {
                let worker = inventory::get_worker(conn, &worker_id, &online_map)?;
                let claimable_task_ids = if worker.approval
                    == myrlet_protocol::WorkerApprovalStatus::Approved
                    && worker.applied_revision.is_some()
                    && worker.applied_revision == worker.reported_revision
                {
                    crate::reads::claimable_for_worker(
                        conn,
                        worker.applied_config.as_ref(),
                        claimable_limit,
                    )?
                } else {
                    Vec::new()
                };
                Ok(WorkerDetail {
                    reported_config: inventory::reported_config(conn, &worker_id)?,
                    worker,
                    claimable_task_ids,
                })
            })
            .await
    }

    /// One Delivery's current Hub result, after verifying Worker ownership.
    pub async fn worker_delivery_result(
        &self,
        worker_id: WorkerId,
        delivery_id: DeliveryId,
    ) -> Result<myrlet_protocol::DeliveryResultView, StoreError> {
        self.db
            .call(move |conn| {
                result::worker_delivery(conn, &worker_id, &delivery_id)?;
                result::delivery_result_view(conn, &delivery_id)?.ok_or(StoreError::NotFound)
            })
            .await
    }
}

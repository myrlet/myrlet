//! Delivery admission, cancellation relay, and restart-safe pending-report flushes.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use myrlet_protocol::{
    CancelState, DeliveryId, ExecutorId, RejectionCode, SlotId, TaskCancelPayload,
    TaskCancelledPayload, TaskId, TaskOfferPayload, TaskRejectedPayload, WorkerWireKind,
};
use myrlet_worker::{ExecutorAdapter, LocalDeliveryRecord, SubmitRequest};

use super::{RuntimeCore, RuntimeError};
use crate::wss::HubSink;

impl RuntimeCore {
    /// Admit one offered Delivery: persist first, then submit at most once.
    pub(super) async fn handle_offer(
        &self,
        sink: &mut HubSink,
        offer: TaskOfferPayload,
    ) -> Result<(), RuntimeError> {
        let shutting_down = self.shutting_down.load(Ordering::SeqCst);
        let adapter = if shutting_down {
            None
        } else {
            self.adapter_for(&offer.slot_id, offer.executor_id.as_ref())
        };
        let rejection = if shutting_down {
            Some(RejectionCode::WorkerShuttingDown)
        } else if adapter.is_none() {
            Some(RejectionCode::ExecutorUnavailable)
        } else {
            None
        };

        let progress_path = self.home.worker_progress_file(&offer.delivery_id)?;
        let mut record = LocalDeliveryRecord::new(
            offer.delivery_id.clone(),
            offer.task_id.clone(),
            offer.slot_id.clone(),
            progress_path.clone(),
        );
        record.executor_id = offer
            .executor_id
            .clone()
            .or_else(|| self.config.default_executor(&offer.slot_id));
        record.rejected = rejection;
        // Durable registration happens before any submission, so a replayed offer for a
        // known `deliveryId` is ignored instead of executed twice.
        if !self.registry.register_if_absent(&record)? {
            tracing::debug!(delivery = %offer.delivery_id, "replayed task.offer ignored");
            return Ok(());
        }
        if let Some(code) = rejection {
            self.send_rejected(sink, &record, code).await?;
            return Ok(());
        }
        let Some(adapter) = adapter else {
            return Ok(());
        };
        let request = SubmitRequest {
            task_id: offer.task_id,
            delivery_id: offer.delivery_id.clone(),
            task: offer.task,
            progress_path,
        };
        match adapter.submit(request, self.on_exit.clone()).await {
            Ok(execution_ref) => {
                self.update(&offer.delivery_id, |record| {
                    record.execution_ref = Some(execution_ref);
                });
            }
            Err(error) => tracing::warn!(
                delivery = %offer.delivery_id,
                executor = %adapter.id(),
                %error,
                "adapter submission failed after registration; the Delivery stays dispatched"
            ),
        }
        Ok(())
    }

    /// Relay a Hub cancellation request and report what the backend answered.
    pub(super) async fn handle_cancel(
        &self,
        sink: &mut HubSink,
        cancel: TaskCancelPayload,
    ) -> Result<(), RuntimeError> {
        let delivery_id = cancel.delivery_id;
        let Some(record) = self.registry.get(&delivery_id)? else {
            return self
                .send_cancelled(
                    sink,
                    &cancel.task_id,
                    &delivery_id,
                    CancelState::Uncertain,
                    cancel.reason,
                )
                .await;
        };
        let state = match (
            record.execution_ref.as_ref(),
            self.adapter_for(&record.slot_id, record.executor_id.as_ref()),
        ) {
            (Some(execution_ref), Some(adapter)) => adapter.cancel(execution_ref).await,
            _ => CancelState::Uncertain,
        };
        let reason = cancel.reason;
        self.update(&delivery_id, |record| {
            record.cancel_report = Some((state, reason.clone()));
        });
        self.send_cancelled(sink, &record.task_id, &delivery_id, state, reason)
            .await
    }

    pub(super) async fn send_rejected(
        &self,
        sink: &mut HubSink,
        record: &LocalDeliveryRecord,
        code: RejectionCode,
    ) -> Result<(), RuntimeError> {
        let payload = TaskRejectedPayload {
            task_id: record.task_id.clone(),
            delivery_id: record.delivery_id.clone(),
            code,
            message: Some(rejection_message(code).to_owned()),
        };
        self.send_frame(
            sink,
            WorkerWireKind::TaskRejected,
            Some(record.slot_id.clone()),
            payload,
        )
        .await
    }

    pub(super) async fn send_cancelled(
        &self,
        sink: &mut HubSink,
        task_id: &TaskId,
        delivery_id: &DeliveryId,
        state: CancelState,
        reason: Option<String>,
    ) -> Result<(), RuntimeError> {
        let payload = TaskCancelledPayload {
            task_id: task_id.clone(),
            delivery_id: delivery_id.clone(),
            state,
            reason,
        };
        self.send_frame(sink, WorkerWireKind::TaskCancelled, None, payload)
            .await
    }

    pub(super) fn mark_rejected_acked(&self, delivery_id: &DeliveryId, recorded: bool) {
        if recorded {
            self.update(delivery_id, |record| record.rejected_acked = true);
        }
    }

    pub(super) fn mark_cancel_acked(&self, delivery_id: &DeliveryId, recorded: bool) {
        if recorded {
            self.update(delivery_id, |record| record.cancel_acked = true);
        }
    }

    /// Adapter for one Delivery, falling back to the Slot's default Executor.
    pub(super) fn adapter_for(
        &self,
        slot_id: &SlotId,
        executor_id: Option<&ExecutorId>,
    ) -> Option<Arc<dyn ExecutorAdapter>> {
        let executor_id = match executor_id {
            Some(executor_id) => executor_id.clone(),
            None => self.config.default_executor(slot_id)?,
        };
        self.adapters.get(&(slot_id.clone(), executor_id)).cloned()
    }

    /// Read-modify-write one Delivery record under the store gate.
    pub(super) fn update(
        &self,
        delivery_id: &DeliveryId,
        mutate: impl FnOnce(&mut LocalDeliveryRecord),
    ) -> Option<LocalDeliveryRecord> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);
        let mut record = self.registry.get(delivery_id).ok().flatten()?;
        mutate(&mut record);
        if let Err(error) = self.registry.save(&record) {
            tracing::warn!(delivery = %delivery_id, %error, "local Delivery record was not saved");
        }
        Some(record)
    }

    /// Deliveries still bound to this Worker: non-rejected and not yet finished.
    pub(super) fn active_delivery_ids(&self) -> Vec<DeliveryId> {
        match self.registry.load_all() {
            Ok(records) => records
                .into_iter()
                .filter(|record| record.rejected.is_none() && !record.finish_acked)
                .map(|record| record.delivery_id)
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "local Delivery registry could not be read");
                Vec::new()
            }
        }
    }

    /// Wait for owned executions to end, then cancel what still runs. Never fabricates a
    /// finish report: an execution that is merely signalled stays honestly unresolved.
    pub(super) async fn drain(&self) {
        let deadline = Instant::now() + Duration::from_millis(self.config.shutdown_timeout_ms());
        loop {
            let pending = self.pending_executions();
            if pending.is_empty() {
                return;
            }
            if Instant::now() >= deadline {
                for record in &pending {
                    self.cancel_execution(record).await;
                }
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn pending_executions(&self) -> Vec<LocalDeliveryRecord> {
        match self.registry.load_all() {
            Ok(records) => records
                .into_iter()
                .filter(|record| {
                    record.rejected.is_none()
                        && record.execution_ref.is_some()
                        && !record.finish_acked
                })
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "local Delivery registry could not be read while draining");
                Vec::new()
            }
        }
    }

    async fn cancel_execution(&self, record: &LocalDeliveryRecord) {
        let Some(execution_ref) = record.execution_ref.as_ref() else {
            return;
        };
        let Some(adapter) = self.adapter_for(&record.slot_id, record.executor_id.as_ref()) else {
            return;
        };
        let state = adapter.cancel(execution_ref).await;
        tracing::info!(
            delivery = %record.delivery_id,
            ?state,
            "execution signalled while shutting down"
        );
    }
}

fn rejection_message(code: RejectionCode) -> &'static str {
    match code {
        RejectionCode::ExecutorUnavailable => "no configured executor handles this slot",
        RejectionCode::WorkerShuttingDown => "the worker is shutting down",
        RejectionCode::RecoveryIncomplete => "the worker is still recovering local deliveries",
    }
}

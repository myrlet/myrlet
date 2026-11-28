//! Wakeup handling, explicit claims, cancellation relay, and restart-safe drains.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use myrlet_protocol::{
    CancelState, ClaimTaskRequest, ClaimTaskResponse, DeliveryId, ExecutorId, SlotId,
    TaskCancelPayload, TaskCancelledPayload, TaskId, WorkerWireKind,
};
use myrlet_worker::{ExecutorAdapter, LocalDeliveryRecord, SubmitRequest};

use super::{RuntimeCore, RuntimeError};
use crate::wss::HubSink;

impl RuntimeCore {
    /// React to one wakeup hint by trying to claim the named Task for a local Slot.
    ///
    /// The hint reserves nothing: this Worker picks a free eligible Slot, asks the Hub to
    /// commit the claim, and only then launches the Harness. Losing the race is normal.
    pub(super) async fn handle_wakeup(&self, task_id: &TaskId) -> Result<(), RuntimeError> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Ok(());
        }
        let Some(slot) = self.claimable_slot() else {
            return Ok(());
        };
        let epoch = self.epoch();
        if epoch == 0 {
            return Ok(());
        }
        let executor_id = self.config.default_executor(&slot.slot_id);
        let request = ClaimTaskRequest {
            task_id: task_id.clone(),
            slot_id: slot.slot_id.clone(),
            connection_epoch: epoch,
            // The Hub resolves the Task's own preferred Executor; the Worker only names
            // the Slot it is claiming, so it never has to second-guess the definition.
            executor_id: None,
        };
        let Some(response) = self.reporter.claim(&request).await? else {
            return Ok(());
        };
        self.admit_claim(response, slot.slot_id, executor_id).await;
        Ok(())
    }

    /// Durably register the committed Delivery, then submit exactly once.
    async fn admit_claim(
        &self,
        response: ClaimTaskResponse,
        slot_id: SlotId,
        executor_id: Option<ExecutorId>,
    ) {
        let delivery = response.delivery;
        let progress_path = match self.home.worker_progress_file(&delivery.id) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(%error, delivery = %delivery.id, "progress path is unusable");
                return;
            }
        };
        let result_path = match self.home.worker_result_file(&delivery.id) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(%error, delivery = %delivery.id, "result path is unusable");
                return;
            }
        };
        let mut record = LocalDeliveryRecord::new(
            delivery.id.clone(),
            delivery.task_id.clone(),
            slot_id.clone(),
            progress_path.clone(),
            result_path.clone(),
        );
        record.executor_id = delivery
            .executor_id
            .clone()
            .or(executor_id)
            .or_else(|| self.config.default_executor(&slot_id));
        // The claim is already committed at the Hub, so registration before submission
        // is what makes a replayed claim reply safe: a known `deliveryId` is never run twice.
        match self.registry.register_if_absent(&record) {
            Ok(true) => {}
            Ok(false) => {
                tracing::debug!(delivery = %delivery.id, "replayed claim reply ignored");
                return;
            }
            Err(error) => {
                tracing::warn!(%error, delivery = %delivery.id, "delivery could not be registered");
                return;
            }
        }
        let Some(adapter) = self.adapter_for(&slot_id, record.executor_id.as_ref()) else {
            tracing::warn!(
                delivery = %delivery.id,
                slot = %slot_id,
                "no configured executor handles the claimed slot"
            );
            return;
        };
        let request = SubmitRequest {
            task_id: delivery.task_id,
            delivery_id: delivery.id.clone(),
            task: response.task.task,
            progress_path,
            result_path,
        };
        match adapter.submit(request, self.on_exit.clone()).await {
            Ok(execution_ref) => {
                self.update(&delivery.id, |record| {
                    record.execution_ref = Some(execution_ref);
                });
            }
            Err(error) => tracing::warn!(
                delivery = %delivery.id,
                executor = %adapter.id(),
                %error,
                "adapter submission failed after registration; the Delivery stays dispatched"
            ),
        }
    }

    /// The first enabled Slot with no active local execution, if any is free.
    fn claimable_slot(&self) -> Option<crate::config::SlotConfig> {
        let busy = self.busy_slots();
        self.config
            .slots
            .iter()
            .filter(|slot| slot.enabled)
            .find(|slot| !busy.contains(&slot.slot_id))
            .cloned()
    }

    /// Slots holding an unfinished local execution.
    fn busy_slots(&self) -> Vec<SlotId> {
        match self.registry.load_all() {
            Ok(records) => records
                .into_iter()
                .filter(|record| !record.finish_acked)
                .map(|record| record.slot_id)
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "local Delivery registry could not be read");
                Vec::new()
            }
        }
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
            return None;
        }
        Some(record)
    }

    /// Deliveries still bound to this Worker: not yet finished.
    pub(super) fn active_delivery_ids(&self) -> Vec<DeliveryId> {
        match self.registry.load_all() {
            Ok(records) => records
                .into_iter()
                .filter(|record| !record.finish_acked)
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
                .filter(|record| record.execution_ref.is_some() && !record.finish_acked)
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

//! Wakeup handling, explicit claims, cancellation relay, and restart-safe drains.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use myrlet_protocol::{
    CancelState, ClaimTaskRequest, ClaimTaskResponse, DeliveryId, ExecutorId, HubWakeupPayload,
    SlotId, TaskCancelPayload, TaskCancelledPayload, TaskId, WorkerWireKind,
};
use myrlet_worker::{ExecutorAdapter, LocalDeliveryRecord, SubmitRequest};

use super::{ClaimAdapters, RuntimeCore, RuntimeError};
use crate::wss::HubSink;

impl RuntimeCore {
    pub(super) async fn handle_wakeup(&self, wakeup: HubWakeupPayload) -> Result<(), RuntimeError> {
        if self.shutting_down.load(Ordering::SeqCst)
            || self.epoch() == 0
            || self.claims_paused.load(Ordering::SeqCst)
        {
            return Ok(());
        }
        if self
            .pending_claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
            || self.claim_in_flight.load(Ordering::SeqCst)
        {
            return Ok(());
        }
        let busy = self.busy_slots()?;
        let slot_id = {
            let state = self.state();
            state
                .config
                .slots
                .iter()
                .find(|slot| {
                    slot.enabled
                        && !slot.executors.is_empty()
                        && wakeup.eligible_slot_ids.contains(&slot.slot_id)
                        && !busy.contains(&slot.slot_id)
                })
                .map(|slot| slot.slot_id.clone())
        };
        let Some(slot_id) = slot_id else {
            return Ok(());
        };
        // Pin this Slot's adapters before the request leaves. The Hub still resolves the
        // Task's preferred Executor; the snapshot is used only to admit the returned
        // Delivery if `worker.yaml` reloads before its response arrives.
        let adapters = {
            let state = self.state();
            let slot = state
                .config
                .slots
                .iter()
                .find(|slot| slot.slot_id == slot_id)
                .expect("the selected Slot is in the live config");
            ClaimAdapters {
                default: state.config.default_executor(&slot_id),
                adapters: slot
                    .executors
                    .iter()
                    .filter_map(|executor| {
                        state
                            .adapters
                            .get(&(slot_id.clone(), executor.id().clone()))
                            .map(|adapter| (executor.id().clone(), Arc::clone(adapter)))
                    })
                    .collect(),
            }
        };
        if adapters.adapters.is_empty() {
            return Ok(());
        }
        *self
            .pending_adapters
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(adapters);
        *self
            .pending_claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(ClaimTaskRequest {
            task_id: wakeup.task_id,
            slot_id,
            connection_epoch: self.epoch(),
            executor_id: None,
        });
        self.retry_pending_claim().await
    }

    /// Only the WSS session drives claims. Until the Hub gives a definite answer,
    /// keep the original Task and Slot and retry with the current connection epoch.
    pub(super) async fn retry_pending_claim(&self) -> Result<(), RuntimeError> {
        let pending = self
            .pending_claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(mut request) = pending else {
            return Ok(());
        };
        request.connection_epoch = self.epoch();
        self.claim_in_flight.store(true, Ordering::SeqCst);
        let response = self.reporter.claim(&request).await;
        self.claim_in_flight.store(false, Ordering::SeqCst);
        match response {
            Ok(Some(response)) => self.admit_claim(response).await,
            Ok(None) => {}
            Err(error)
                if error.is_retryable()
                    || matches!(error, crate::reporting::ReportError::StaleDelivery(_)) =>
            {
                return Err(error.into())
            }
            Err(error) => tracing::warn!(%error, task_id = %request.task_id, "claim was refused"),
        }
        *self
            .pending_claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        Ok(())
    }

    /// Durably register the committed Delivery, then submit exactly once.
    async fn admit_claim(&self, response: ClaimTaskResponse) {
        let delivery = response.delivery;
        let slot_id = delivery.slot_id.clone();
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
        let pending_adapters = self
            .pending_adapters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        record.executor_id = delivery.executor_id.clone().or_else(|| {
            pending_adapters
                .as_ref()
                .and_then(ClaimAdapters::executor_id)
        });
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
        let adapter = record
            .executor_id
            .as_ref()
            .and_then(|executor_id| {
                pending_adapters
                    .as_ref()
                    .and_then(|adapters| adapters.get(executor_id))
            })
            .or_else(|| self.adapter_for(&delivery.id, &slot_id, record.executor_id.as_ref()));
        let Some(adapter) = adapter else {
            tracing::warn!(
                delivery = %delivery.id,
                slot = %slot_id,
                "no configured executor handles the claimed slot"
            );
            return;
        };
        // Pin the adapter that admits this Delivery, so a later `worker.yaml` reload keeps
        // its cancellation path even when the Slot's configured Executor disappears.
        self.delivery_adapters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(delivery.id.clone(), Arc::clone(&adapter));
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

    /// Slots holding an unfinished local execution.
    fn busy_slots(&self) -> Result<Vec<SlotId>, RuntimeError> {
        Ok(self
            .registry
            .load_all()?
            .into_iter()
            .filter(|record| !record.finish_acked)
            .map(|record| record.slot_id)
            .collect())
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
            self.adapter_for(&delivery_id, &record.slot_id, record.executor_id.as_ref()),
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

    /// Adapter for one Delivery: the one that admitted it, else the Slot's configured
    /// default Executor.
    pub(super) fn adapter_for(
        &self,
        delivery_id: &DeliveryId,
        slot_id: &SlotId,
        executor_id: Option<&ExecutorId>,
    ) -> Option<Arc<dyn ExecutorAdapter>> {
        if let Some(adapter) = self
            .delivery_adapters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(delivery_id)
        {
            return Some(Arc::clone(adapter));
        }
        let executor_id = match executor_id {
            Some(executor_id) => executor_id.clone(),
            None => {
                let state = self.state();
                return state
                    .config
                    .default_executor(slot_id)
                    .and_then(|id| state.adapters.get(&(slot_id.clone(), id)).cloned());
            }
        };
        self.state()
            .adapters
            .get(&(slot_id.clone(), executor_id))
            .cloned()
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
        let deadline = Instant::now() + Duration::from_millis(self.shutdown_timeout_ms());
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
        let Some(adapter) = self.adapter_for(
            &record.delivery_id,
            &record.slot_id,
            record.executor_id.as_ref(),
        ) else {
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

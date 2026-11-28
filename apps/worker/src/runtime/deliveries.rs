//! Wakeup handling, explicit claims, cancellation relay, and restart-safe drains.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use myrlet_protocol::{
    CancelState, ClaimTaskRequest, ClaimTaskResponse, DeliveryId, ExecutorId, HubWakeupPayload,
    SlotId, TaskCancelPayload, TaskCancelledPayload, TaskId, WorkerWireKind,
};
use myrlet_worker::process_group::CANCEL_ACK_BOUND;
use myrlet_worker::{ExecutorAdapter, LocalDeliveryRecord, SubmitRequest};

use super::{RuntimeCore, RuntimeError};
use crate::wss::HubSink;

impl RuntimeCore {
    pub(super) async fn handle_wakeup(&self, wakeup: HubWakeupPayload) -> Result<(), RuntimeError> {
        if self.shutting_down.load(Ordering::SeqCst) || self.epoch() == 0 {
            return Ok(());
        }
        if self
            .pending_claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
        {
            return Ok(());
        }
        let busy = self.busy_slots()?;
        let slot = self.config.slots.iter().find(|slot| {
            slot.enabled
                && !slot.executors.is_empty()
                && wakeup.eligible_slot_ids.contains(&slot.slot_id)
                && !busy.contains(&slot.slot_id)
        });
        let Some(slot) = slot else {
            return Ok(());
        };
        *self
            .pending_claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(ClaimTaskRequest {
            task_id: wakeup.task_id,
            slot_id: slot.slot_id.clone(),
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
        let response = self.reporter.claim(&request).await;
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
        record.executor_id = delivery
            .executor_id
            .clone()
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
    ///
    /// A repeated request (a reconnect replay or a duplicate frame) re-sends the answer
    /// already recorded for this Delivery instead of cancelling again: the execution handle
    /// is gone, so a second attempt could only produce `Uncertain` and would weaken a
    /// truthful `Confirmed` into a false negative.
    ///
    /// Cleanup can take several seconds, so the WSS heartbeat keeps flowing while the
    /// adapter works; otherwise the Hub lease would expire mid-cancel and the report would
    /// be lost with the connection.
    pub(super) async fn handle_cancel(
        &self,
        sink: &mut HubSink,
        cancel: TaskCancelPayload,
        heartbeat: Duration,
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
        let state = match record.cancel_report.clone() {
            Some((state, _)) => state,
            None => match (
                record.execution_ref.as_ref(),
                self.adapter_for(&record.slot_id, record.executor_id.as_ref()),
            ) {
                (Some(execution_ref), Some(adapter)) => {
                    self.cancel_while_heartbeating(sink, adapter.as_ref(), execution_ref, heartbeat)
                        .await?
                }
                _ => CancelState::Uncertain,
            },
        };
        let reason = cancel.reason;
        self.update(&delivery_id, |record| {
            record.cancel_report = Some((state, reason.clone()));
        });
        self.send_cancelled(sink, &record.task_id, &delivery_id, state, reason)
            .await
    }

    /// Run one adapter cancel to completion while keeping the Hub lease alive.
    async fn cancel_while_heartbeating(
        &self,
        sink: &mut HubSink,
        adapter: &dyn ExecutorAdapter,
        execution_ref: &myrlet_protocol::ExecutionRef,
        heartbeat: Duration,
    ) -> Result<CancelState, RuntimeError> {
        let cancel = adapter.cancel(execution_ref);
        tokio::pin!(cancel);
        loop {
            tokio::select! {
                state = &mut cancel => return Ok(state),
                () = tokio::time::sleep(heartbeat) => self.heartbeat_now(sink).await?,
            }
        }
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
    ///
    /// The wait is for **local handling to end**, not for the Hub to acknowledge a Finish.
    /// Once the WSS session is gone the epoch is zero, so a pending Finish cannot be posted
    /// at all; waiting for `finish_acked` here would stall for the whole shutdown budget.
    ///
    /// Cancellation starts early enough for its acknowledgement bound to fit inside
    /// `MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS`, so a normal budget both drains in-flight work
    /// and reaps owned process groups before the operator's bound. When the configured
    /// bound is shorter than cleanup needs, the cancel wait is **not** truncated: returning
    /// early would let the process exit mid-cleanup and orphan a TERM-ignoring group.
    /// `main` therefore leaves the runtime this much margin past its own bound.
    pub(super) async fn drain(&self) {
        let budget = Duration::from_millis(self.config.shutdown_timeout_ms());
        let cancel_from = tokio::time::Instant::now() + budget.saturating_sub(CANCEL_ACK_BOUND);
        loop {
            let pending = self.running_executions();
            if pending.is_empty() {
                return;
            }
            if tokio::time::Instant::now() >= cancel_from {
                // Every owned execution is cancelled together, so the wait is bounded by
                // one acknowledgement rather than growing with the number of Slots.
                futures::future::join_all(
                    pending.iter().map(|record| self.cancel_execution(record)),
                )
                .await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Deliveries whose adapter execution is still owned locally: not ended and not
    /// cancelled. Hub acknowledgement is deliberately not part of this condition.
    fn running_executions(&self) -> Vec<LocalDeliveryRecord> {
        match self.registry.load_all() {
            Ok(records) => records
                .into_iter()
                .filter(|record| {
                    record.execution_ref.is_some()
                        && !record.finish_pending
                        && !record.finish_acked
                        && record.cancel_report.is_none()
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
            "owned execution cancelled while shutting down"
        );
    }
}

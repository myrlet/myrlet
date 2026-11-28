//! Cancellation relay, adapter resolution, and restart-safe drains.
//!
//! Every committed Delivery keeps the adapter that admitted it in `delivery_adapters`, so a
//! `worker.yaml` reload never strands a running execution's cancellation or reporting path.
//! That registry is bounded by the Delivery's own lifecycle: a reference is released when
//! the Delivery finishes or is cancelled, and never grows with completed work.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use myrlet_protocol::{
    CancelState, DeliveryId, ExecutorId, SlotId, TaskCancelPayload, TaskCancelledPayload, TaskId,
    WorkerWireKind,
};
use myrlet_worker::process_group::CANCEL_ACK_BOUND;
use myrlet_worker::{ExecutorAdapter, LocalDeliveryRecord};

use super::{RuntimeCore, RuntimeError};
use crate::wss::HubSink;

impl RuntimeCore {
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
                self.adapter_for(&delivery_id, &record.slot_id, record.executor_id.as_ref()),
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

    /// The Hub stored a cancel report; a Delivery whose handling has ended no longer needs
    /// its pinned adapter.
    pub(super) fn mark_cancel_acked(&self, delivery_id: &DeliveryId, recorded: bool) {
        if !recorded {
            return;
        }
        if let Some(record) = self.update(delivery_id, |record| record.cancel_acked = true) {
            if record_is_terminal(&record) {
                self.release_adapter(delivery_id);
            }
        }
    }

    /// A Hub-confirmed Finish, or a replayed one, ends this Delivery's adapter need.
    pub(super) fn release_finished_adapter(&self, delivery_id: &DeliveryId) {
        if let Some(record) = self.registry.get(delivery_id).ok().flatten() {
            if record.finish_acked {
                self.release_adapter(delivery_id);
            }
        }
    }

    /// Drop one pinned adapter; the Delivery no longer has a live execution to cancel.
    fn release_adapter(&self, delivery_id: &DeliveryId) {
        let released = self
            .delivery_adapters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(delivery_id);
        if released.is_some() {
            tracing::debug!(delivery = %delivery_id, "released the pinned Delivery adapter");
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
        let budget = Duration::from_millis(self.shutdown_timeout_ms());
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
            "owned execution cancelled while shutting down"
        );
    }
}

/// A Delivery whose handling ended: the Hub accepted its Finish, or the cancellation was
/// acknowledged. Its pinned adapter is no longer needed for cancellation.
fn record_is_terminal(record: &LocalDeliveryRecord) -> bool {
    record.finish_acked || (record.cancel_report.is_some() && record.cancel_acked)
}

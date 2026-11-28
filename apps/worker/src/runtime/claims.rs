//! Wakeup handling and Claim admission: select a Slot, pin its adapters, submit once.
//!
//! Slot selection and adapter capture happen under one read of the live configuration, so
//! a concurrent `worker.yaml` reload can replace the configuration between the two without
//! leaving a selected Slot that no longer exists. Adapter references are pinned before the
//! claim leaves: a reload that removes the Slot or Executor cannot strand the Delivery's
//! execution, cancellation, or reporting path.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use myrlet_protocol::{ClaimTaskRequest, ClaimTaskResponse, ExecutorId, HubWakeupPayload, SlotId};
use myrlet_worker::{ExecutorAdapter, LocalDeliveryRecord, SubmitRequest};

use super::{ClaimAdapters, RuntimeCore, RuntimeError};

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
            || self.claim_in_flight.load(Ordering::SeqCst)
        {
            return Ok(());
        }
        let busy = self.busy_slots()?;
        // Read the configuration once: the Slot and the adapters that would admit its
        // Delivery must describe the same snapshot. Holding this guard across the capture
        // is fine because nothing in it awaits; it is released before the claim leaves.
        let selected = {
            let state = self.state();
            if state.inventory_pending {
                return Ok(()); // claims stay paused until the Hub acknowledges the report
            }
            select_slot(&state, &wakeup, &busy)
        };
        let Some((slot_id, adapters)) = selected else {
            return Ok(());
        };
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
                // The outcome is unknown: keep the same Task, Slot, and pinned adapters so
                // the session replays this claim against the current epoch.
                return Err(error.into());
            }
            Err(error) => tracing::warn!(%error, task_id = %request.task_id, "claim was refused"),
        }
        // A definite answer (admitted, lost the race, or refused) resolves the claim.
        self.clear_pending_claim();
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
        // its cancellation path even when the Slot's configured Executor disappears. The
        // reference is released once the Delivery finishes or is cancelled.
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

    /// A Claim resolved without a Delivery: release the pinned adapters.
    fn clear_pending_claim(&self) {
        *self
            .pending_claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        *self
            .pending_adapters
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
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
}

/// Select one eligible, idle Slot and capture the adapters that would admit its Delivery.
///
/// The Slot chosen and the adapters returned describe the same configuration read, so a
/// concurrent reload can never leave a selected Slot without its adapters. `None` means no
/// hinted Slot is usable right now.
fn select_slot(
    state: &super::RuntimeState,
    wakeup: &HubWakeupPayload,
    busy: &[SlotId],
) -> Option<(SlotId, ClaimAdapters)> {
    let slot = state.config.slots.iter().find(|slot| {
        slot.enabled
            && !slot.executors.is_empty()
            && wakeup.eligible_slot_ids.contains(&slot.slot_id)
            && !busy.contains(&slot.slot_id)
    })?;
    let slot_id = slot.slot_id.clone();
    let adapters = ClaimAdapters {
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
            .collect::<std::collections::HashMap<ExecutorId, Arc<dyn ExecutorAdapter>>>(),
    };
    (!adapters.adapters.is_empty()).then_some((slot_id, adapters))
}

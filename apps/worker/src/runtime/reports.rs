//! Background reporting: end-of-handling → HTTP `/finish` and Progress file polls →
//! HTTP `/progress`. Both wait for a live non-zero `connectionEpoch`, and neither ever
//! writes the Progress file or invents a terminal verdict.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use myrlet_protocol::ProgressReportRequest;
use myrlet_worker::read_progress_if_changed;
use tokio::task::JoinHandle;

use super::{RuntimeCore, RuntimeError, PROGRESS_POLL};
use crate::wss::HubSink;

impl RuntimeCore {
    /// Spawn the exit handler and the Progress poller for this runtime.
    pub(super) fn spawn_reports(self: Arc<Self>) -> (JoinHandle<()>, JoinHandle<()>) {
        let exit = tokio::spawn({
            let core = Arc::clone(&self);
            async move { core.run_exit_handler().await }
        });
        let progress = tokio::spawn({
            let core = Arc::clone(&self);
            async move { core.run_progress_loop().await }
        });
        (exit, progress)
    }

    /// Flush unacknowledged refusals, cancel reports, and pending finishes after resync.
    pub(super) async fn flush_pending(&self, sink: &mut HubSink) -> Result<(), RuntimeError> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        if epoch == 0 {
            return Ok(());
        }
        for record in self.registry.load_all()? {
            if let Some(code) = record.rejected {
                if !record.rejected_acked {
                    self.send_rejected(sink, &record, code).await?;
                }
            }
            if let Some((state, reason)) = record.cancel_report.clone() {
                if !record.cancel_acked {
                    self.send_cancelled(sink, &record.task_id, &record.delivery_id, state, reason)
                        .await?;
                }
            }
            if record.finish_pending && !record.finish_acked {
                match self.reporter.post_finish(&record.delivery_id, epoch).await {
                    Ok(Some(_)) => {
                        self.update(&record.delivery_id, |record| record.finish_acked = true);
                    }
                    Ok(None) => {}
                    Err(error) => tracing::warn!(
                        delivery = %record.delivery_id,
                        %error,
                        "pending finish report could not be flushed"
                    ),
                }
            }
        }
        Ok(())
    }

    /// Record end of handling, relay any final Progress, then post `/finish` once an epoch is live.
    async fn run_exit_handler(self: Arc<Self>) {
        let receiver = self
            .on_exit_rx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(mut receiver) = receiver else {
            return;
        };
        while let Some(ended) = receiver.recv().await {
            let Some(record) = self.update(&ended.delivery_id, |record| {
                record.execution_ref = Some(ended.execution_ref.clone());
                record.finish_pending = true;
            }) else {
                continue;
            };
            let epoch = self.epoch.load(Ordering::SeqCst);
            if epoch == 0 {
                continue;
            }
            self.relay_progress_once(&record, epoch).await;
            self.post_finish_if_pending(&record.delivery_id, epoch)
                .await;
        }
    }

    /// Relay newly observed Progress and retry deferred finishes for active Deliveries.
    async fn run_progress_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(PROGRESS_POLL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let epoch = self.epoch.load(Ordering::SeqCst);
            if epoch == 0 {
                continue;
            }
            let records = match self.registry.load_all() {
                Ok(records) => records,
                Err(error) => {
                    tracing::warn!(%error, "local Delivery registry could not be read");
                    continue;
                }
            };
            for record in records {
                if record.rejected.is_some() || record.finish_acked {
                    continue;
                }
                self.relay_progress_once(&record, epoch).await;
                if record.finish_pending {
                    self.post_finish_if_pending(&record.delivery_id, epoch)
                        .await;
                }
            }
        }
    }

    async fn post_finish_if_pending(&self, delivery_id: &myrlet_protocol::DeliveryId, epoch: u64) {
        match self.reporter.post_finish(delivery_id, epoch).await {
            Ok(Some(_)) => {
                self.update(delivery_id, |record| record.finish_acked = true);
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(
                delivery = %delivery_id,
                %error,
                "finish report deferred until the next poll or resync"
            ),
        }
    }

    async fn relay_progress_once(&self, record: &myrlet_worker::LocalDeliveryRecord, epoch: u64) {
        let mut hash = self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&record.delivery_id)
            .copied();
        let Some(observed) = read_progress_if_changed(
            &record.progress_path,
            &mut hash,
            record.last_progress_observed_ms.unwrap_or(0),
        ) else {
            return;
        };
        let observed_at_ms = observed.observed_at_ms;
        let request = ProgressReportRequest {
            epoch,
            observed_at_ms,
            text: observed.text,
        };
        match self
            .reporter
            .post_progress(&record.delivery_id, &request)
            .await
        {
            Ok(_) => {
                if let Some(hash) = hash {
                    self.progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(record.delivery_id.clone(), hash);
                }
                self.update(&record.delivery_id, |record| {
                    record.last_progress_observed_ms = Some(observed_at_ms);
                });
            }
            Err(error) => tracing::debug!(
                delivery = %record.delivery_id,
                %error,
                "progress report not yet accepted; will retry"
            ),
        }
    }
}

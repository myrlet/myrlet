//! Background reconciliation: Progress file polls, Result file polls, and the exit-driven
//! Finish. Each report is independent, waits for a live non-zero `connectionEpoch`, and
//! never writes a Harness-owned file or invents a terminal verdict.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use myrlet_protocol::{
    result_sha256, DeliveryId, ProgressReportRequest, ResultReceipt, ResultReportRequest,
};
use myrlet_worker::{read_progress_if_changed, read_result, LocalDeliveryRecord, LocalResultError};
use tokio::task::JoinHandle;

use super::{RuntimeCore, RuntimeError, REPORT_POLL};
use crate::reporting::ReportError;
use crate::wss::HubSink;

impl RuntimeCore {
    /// Spawn the exit handler and the Progress/Result poller for this runtime.
    pub(super) fn spawn_reports(self: Arc<Self>) -> (JoinHandle<()>, JoinHandle<()>) {
        let exit = tokio::spawn({
            let core = Arc::clone(&self);
            async move { core.run_exit_handler().await }
        });
        let poller = tokio::spawn({
            let core = Arc::clone(&self);
            async move { core.run_report_loop().await }
        });
        (exit, poller)
    }

    /// Flush unacknowledged cancel reports and pending finishes after resync, then
    /// reconcile both independent values.
    pub(super) async fn flush_pending(&self, sink: &mut HubSink) -> Result<(), RuntimeError> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        if epoch == 0 {
            return Ok(());
        }
        for record in self.registry.load_all()? {
            if let Some((state, reason)) = record.cancel_report.clone() {
                if !record.cancel_acked {
                    self.send_cancelled(sink, &record.task_id, &record.delivery_id, state, reason)
                        .await?;
                }
            }
            self.reconcile(&record.delivery_id, epoch).await;
        }
        Ok(())
    }

    /// Record end of handling, relay the final Progress, Finish, and Result.
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
            self.reconcile(&record.delivery_id, epoch).await;
        }
    }

    /// Poll every retained Delivery for changed Progress and Result content.
    async fn run_report_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(REPORT_POLL);
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
                self.reconcile(&record.delivery_id, epoch).await;
            }
        }
    }

    /// Serialize all Progress/Result reconciliation of one Delivery behind its own lock.
    ///
    /// The poll loop, the exit handler, resync flush, and a manual `result sync` command
    /// all funnel here, so no two of them ever have a request for the same Delivery in
    /// flight at once. The lock is per Delivery, so unrelated Deliveries stay parallel.
    async fn reconcile(&self, delivery_id: &myrlet_protocol::DeliveryId, epoch: u64) {
        let lock = self.reconciliation_lock(delivery_id);
        let _guard = lock.lock().await;
        if self.epoch() != epoch {
            return;
        }
        let Some(record) = self.registry.get(delivery_id).ok().flatten() else {
            return;
        };
        if record.finish_pending && !record.finish_acked {
            self.post_finish_if_pending(delivery_id, epoch).await;
        }
        self.sync_progress(&record, epoch).await;
        self.sync_result(&record, epoch).await;
    }

    fn reconciliation_lock(
        &self,
        delivery_id: &myrlet_protocol::DeliveryId,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .reconciliation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Arc::clone(
            locks
                .entry(delivery_id.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
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

    /// Relay Progress only when the file's text differs from what the Hub last confirmed.
    async fn sync_progress(&self, record: &LocalDeliveryRecord, epoch: u64) {
        let mut last_hash = record.progress_acked_sha256.as_deref().and_then(hex_hash);
        let Some(observed) = read_progress_if_changed(
            &record.progress_path,
            &mut last_hash,
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
                self.update(&record.delivery_id, |record| {
                    record.last_progress_observed_ms = Some(observed_at_ms);
                    record.progress_acked_sha256 = Some(result_sha256(&request.text));
                });
            }
            Err(error) => tracing::debug!(
                delivery = %record.delivery_id,
                %error,
                "progress report not yet accepted; will retry"
            ),
        }
    }

    /// Reconcile the Hub Result with the current local content.
    ///
    /// The file is reread on every attempt. Equal content needs no write. A complete local
    /// recovery that finds no or empty content clears the Hub copy; an unreadable file is
    /// never treated as absence, so the Hub value survives.
    async fn sync_result(&self, record: &LocalDeliveryRecord, epoch: u64) {
        let local = read_result(&record.result_path);
        let wanted = match local.content() {
            Ok(text) => text.map(str::to_owned),
            Err(LocalResultError::Incomplete) => return,
        };
        if !record.result_recovery_incomplete
            && wanted.as_deref().map(result_sha256) == record.result_acked_sha256
        {
            return;
        }
        let request = ResultReportRequest {
            epoch,
            text: wanted,
        };
        if let Err(error) = self.write_result(&record.delivery_id, &request).await {
            tracing::debug!(delivery = %record.delivery_id, %error, "result report not yet accepted; will retry");
        }
    }

    /// Both Result writers use the Delivery lock before reaching this write boundary.
    async fn write_result(
        &self,
        delivery_id: &DeliveryId,
        request: &ResultReportRequest,
    ) -> Result<ResultReceipt, ReportError> {
        let epoch = request.epoch;
        if self.epoch() != epoch {
            return Err(ReportError::StaleDelivery(delivery_id.to_string()));
        }
        match self.reporter.post_result(delivery_id, request).await {
            Ok(receipt) => {
                self.update(delivery_id, |record| {
                    record.result_acked_sha256 = receipt.result_sha256.clone();
                    record.result_recovery_incomplete = false;
                });
                Ok(receipt)
            }
            Err(error) => {
                if matches!(error, ReportError::Transport(_))
                    || matches!(error, ReportError::Response { status, .. } if status == 408 || status >= 500)
                {
                    // The write may have committed. The previously acknowledged hash
                    // no longer establishes what the Hub holds, even if the file reverts.
                    self.update(delivery_id, |record| {
                        record.result_recovery_incomplete = true
                    });
                    if self
                        .epoch
                        .compare_exchange(epoch, 0, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        tracing::warn!(%delivery_id, epoch, "result write outcome unknown; reconnecting before retry");
                        self.reconnect.notify_one();
                    }
                }
                Err(error)
            }
        }
    }

    /// The Hub's current Result digest for one owned Delivery; `Ok(None)` means no Result.
    pub(super) async fn hub_result_sha(
        &self,
        delivery_id: &myrlet_protocol::DeliveryId,
    ) -> Result<Option<String>, crate::reporting::ReportError> {
        let view = self.reporter.get_result(delivery_id).await?;
        Ok(view.result_sha256)
    }

    /// Force one reconciliation and return the digest the Hub now holds.
    pub(super) async fn sync_result_now(
        &self,
        delivery_id: &myrlet_protocol::DeliveryId,
    ) -> Result<Option<String>, crate::reporting::ReportError> {
        let lock = self.reconciliation_lock(delivery_id);
        let _guard = lock.lock().await;
        let Some(record) = self
            .registry
            .get(delivery_id)
            .map_err(|error| crate::reporting::ReportError::Transport(error.to_string()))?
        else {
            return Ok(None);
        };
        let epoch = self.epoch.load(Ordering::SeqCst);
        if epoch == 0 {
            return Err(crate::reporting::ReportError::Transport(
                "no live Hub connection".into(),
            ));
        }
        let local = read_result(&record.result_path);
        let wanted = local
            .content()
            .map_err(|_| crate::reporting::ReportError::Transport("incomplete local read".into()))?
            .map(str::to_owned);
        let request = ResultReportRequest {
            epoch,
            text: wanted,
        };
        let receipt = self.write_result(delivery_id, &request).await?;
        Ok(receipt.result_sha256)
    }
}

/// Parse a stored lowercase hex digest back into the byte array the watcher tracks.
fn hex_hash(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        bytes[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(bytes)
}

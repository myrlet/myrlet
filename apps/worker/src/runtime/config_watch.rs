//! Live `worker.yaml` watching: validate a replacement before activating it.
//!
//! An invalid edit keeps the last valid runtime configuration and logs a local diagnosis.
//! A valid scheduling change pauses new claims, swaps the adapters and reported inventory,
//! and asks the WSS session to report it. The session resumes claim handling once the Hub
//! acknowledges that revision. Existing Deliveries keep their registered adapter.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use crate::config::WorkerConfig;

use super::RuntimeCore;

const CONFIG_POLL: Duration = Duration::from_secs(1);

/// One stable identity for the file contents currently loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

impl RuntimeCore {
    /// Poll `worker.yaml` until the process exits.
    pub(super) async fn watch_config(self: std::sync::Arc<Self>) {
        let path = self.home.worker_config();
        let mut last = stamp(&path);
        let mut ticker = tokio::time::interval(CONFIG_POLL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let current = stamp(&path);
            if current == last {
                continue;
            }
            last = current;
            // Only a changed file is reread, so an editor's atomic replace is seen once and
            // an invalid edit is diagnosed without retrying the same bytes.
            match WorkerConfig::load(&path) {
                Ok(config) => self.activate(config),
                Err(error) => tracing::warn!(
                    %error,
                    path = %path.display(),
                    "worker.yaml is invalid; keeping the last valid runtime configuration"
                ),
            }
        }
    }

    /// Activate one validated replacement, reporting it when the scheduling content changed.
    fn activate(&self, config: WorkerConfig) {
        let current_hub = self.hub_url();
        if config.hub_url != current_hub {
            tracing::warn!(
                hub = %config.hub_url,
                current = %current_hub,
                "a live hubUrl change is ignored; restart the Worker to connect elsewhere"
            );
        }
        let fingerprint = config.inventory_fingerprint();
        // The persisted revision changes only when the scheduling fingerprint does.
        let revision = match self.registry.load_or_bump_inventory_revision(&fingerprint) {
            Ok(revision) => revision,
            Err(error) => {
                tracing::warn!(%error, "a reloaded inventory revision could not be persisted");
                return;
            }
        };
        let inventory = config.build_inventory(revision, crate::now_ms());
        let adapters = config.build_adapters();
        let mut next_config = config;
        next_config.hub_url = current_hub; // the live connection target never moves
        let scheduling_changed = {
            let mut state = self
                .state
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let changed = state.inventory.revision != revision
                || state.config.inventory_fingerprint() != fingerprint;
            state.config = next_config;
            state.inventory = inventory;
            state.adapters = adapters;
            changed
        };
        if !scheduling_changed {
            tracing::info!("worker.yaml reloaded; the reviewed inventory is unchanged");
            return;
        }
        tracing::info!(
            revision,
            "worker.yaml scheduling change activated; pausing new claims"
        );
        if self.claim_in_flight.load(Ordering::SeqCst) {
            tracing::warn!(
                revision,
                "worker.yaml changed while a claim is in flight; the response will use the claimed adapter"
            );
        }
        self.claims_paused.store(true, Ordering::SeqCst);
        self.inventory_pending.store(true, Ordering::SeqCst);
        self.inventory_changed.notify_one();
    }

    /// The Hub acknowledged the inventory this Worker currently reports.
    pub(super) fn inventory_acknowledged(&self, revision: u64) {
        if revision != self.inventory().revision {
            return;
        }
        self.inventory_pending.store(false, Ordering::SeqCst);
        if self.claims_paused.swap(false, Ordering::SeqCst) {
            tracing::info!(revision, "Hub acknowledged the inventory; resuming claims");
        }
    }

    /// Whether a locally reloaded inventory still awaits acknowledgement.
    pub(super) fn inventory_pending(&self) -> bool {
        self.inventory_pending.load(Ordering::SeqCst)
    }
}

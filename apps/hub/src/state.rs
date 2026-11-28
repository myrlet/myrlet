//! Shared Hub state: store handle, credentials, live Worker sessions, and event fan-out.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use myrlet_protocol::{HubEventEnvelope, WorkerId};
use myrlet_store::HubStore;
use tokio::sync::{broadcast, mpsc, Semaphore};

use crate::auth::AdminVerifier;
use crate::config::HubConfig;
use crate::wakeup::WakeupRotation;

/// Capacity of the in-process Hub event broadcast channel.
const EVENTS_CHANNEL_CAPACITY: usize = 1024;
/// Capacity of the Worker-authorization invalidation channel; only wakeups, never payloads.
const AUTH_INVALIDATIONS_CAPACITY: usize = 256;
/// Outbound frame queue depth of one live Worker session.
pub const CONNECTION_QUEUE: usize = 256;

/// One Worker whose global-read authorization may have changed (tier, revocation, rotation).
///
/// Streams subscribe to this signal so they can revalidate their original credential
/// without polling SQLite on every emitted event.
pub type AuthInvalidations = broadcast::Sender<WorkerId>;

/// One live Worker WSS session bound to a single `connectionEpoch`.
pub struct LiveWorker {
    pub connection_id: String,
    pub epoch: u64,
    pub tx: mpsc::Sender<String>,
}

/// Cloneable Hub state shared by every HTTP and WSS handler.
#[derive(Clone)]
pub struct HubState {
    pub hub_id: String,
    pub store: Arc<HubStore>,
    pub admin: AdminVerifier,
    pub events_tx: broadcast::Sender<HubEventEnvelope>,
    /// Fired after a Worker's trust state changes, so open streams revalidate lazily.
    pub auth_invalidations: AuthInvalidations,
    pub connections: Arc<RwLock<HashMap<WorkerId, LiveWorker>>>,
    pub command_permits: Arc<Semaphore>,
    pub shutting_down: Arc<AtomicBool>,
    pub config: HubConfig,
    /// In-memory rotation cursor for wakeup hints; never persisted.
    pub(crate) wakeup_rotation: Arc<WakeupRotation>,
}

impl HubState {
    /// Build the shared state, including the constant-time Admin verifier.
    pub fn new(config: HubConfig, store: Arc<HubStore>, hub_id: String) -> Self {
        let (events_tx, _) = broadcast::channel(EVENTS_CHANNEL_CAPACITY);
        let (auth_invalidations, _) = broadcast::channel(AUTH_INVALIDATIONS_CAPACITY);
        Self {
            hub_id,
            store,
            admin: AdminVerifier::new(&config.admin_token),
            events_tx,
            auth_invalidations,
            connections: Arc::default(),
            command_permits: Arc::new(Semaphore::new(config.max_concurrent_requests.max(1))),
            shutting_down: Arc::new(AtomicBool::new(false)),
            config,
            wakeup_rotation: Arc::new(WakeupRotation::default()),
        }
    }

    /// Fan one committed event out to live subscribers; no receivers is not an error.
    pub fn publish_event(&self, event: Option<HubEventEnvelope>) {
        if let Some(event) = event {
            // A trust-state change must wake open streams so they revalidate promptly; the
            // event itself is still delivered for the normal invalidation path.
            if let Some(worker_id) = worker_trust_subject(&event) {
                let _ = self.auth_invalidations.send(worker_id);
            }
            let _ = self.events_tx.send(event);
        }
    }

    /// Live `(workerId, connectionEpoch)` map used to derive Worker connectivity.
    pub fn online_map(&self) -> HashMap<WorkerId, u64> {
        match self.connections.read() {
            Ok(guard) => guard
                .iter()
                .map(|(id, live)| (id.clone(), live.epoch))
                .collect(),
            Err(_) => HashMap::new(),
        }
    }

    /// Install the newest session for a Worker; any older session is dropped (fenced).
    pub fn register_connection(&self, worker_id: WorkerId, live: LiveWorker) {
        if let Ok(mut guard) = self.connections.write() {
            let _previous = guard.insert(worker_id, live);
        }
    }

    /// Remove a session only while it is still the session for `epoch`.
    pub fn unregister_connection(&self, worker_id: &WorkerId, epoch: u64) {
        if let Ok(mut guard) = self.connections.write() {
            if guard.get(worker_id).is_some_and(|live| live.epoch == epoch) {
                guard.remove(worker_id);
            }
        }
    }

    /// Drop a Worker's session so its WSS loop closes cleanly.
    pub fn disconnect_worker(&self, worker_id: &WorkerId) {
        let _removed = match self.connections.write() {
            Ok(mut guard) => guard.remove(worker_id),
            Err(_) => None,
        };
    }

    /// Queue one frame for a current-epoch session; `false` means no bytes were handed off.
    pub fn send_to_worker(&self, worker_id: &WorkerId, epoch: u64, frame: String) -> bool {
        let Ok(guard) = self.connections.read() else {
            return false;
        };
        match guard.get(worker_id) {
            Some(live) if live.epoch == epoch => live.tx.try_send(frame).is_ok(),
            _ => false,
        }
    }

    /// Whether the Hub has stopped admitting new mutations.
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }
}

/// Current Unix time in milliseconds.
pub fn now_ms() -> i64 {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    i64::try_from(millis).unwrap_or(i64::MAX)
}

/// Event kinds that can change a Worker's trust state or global-read authorization.
const TRUST_EVENT_KINDS: &[&str] = &[
    "worker.permission_tier",
    "worker.revoked",
    "worker.rotated",
    "worker.paired",
    "worker.approved",
];

/// The `workerId` behind a trust-state event, or `None` for ordinary Task events.
fn worker_trust_subject(event: &HubEventEnvelope) -> Option<WorkerId> {
    if !TRUST_EVENT_KINDS.contains(&event.kind.as_str()) {
        return None;
    }
    event
        .payload
        .get("workerId")
        .and_then(serde_json::Value::as_str)
        .map(WorkerId::new)
}

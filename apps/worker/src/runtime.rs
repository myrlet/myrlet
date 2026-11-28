//! Worker daemon runtime: outbound WSS session, local Delivery registry, HTTP reporting.
//!
//! Invariants: a claimed Delivery is registered durably **before** the Harness starts, a
//! replayed claim reply never runs twice, a post-registration submit failure stays
//! unreported, no terminal verdict is ever fabricated, and no Harness-owned file is ever
//! written here.
//!
//! `worker.yaml` is watched locally. A valid scheduling change pauses new claims, swaps the
//! adapters, reports the new inventory, and resumes once the Hub acknowledges it. Every
//! committed Delivery keeps the adapter that admitted it, so a config change never loses a
//! running execution, its cancellation path, or its reporting path.

mod claims;
mod config_watch;
mod deliveries;
mod local;
mod reports;
mod session;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use myrlet_protocol::inventory::WorkerInventory;
use myrlet_protocol::paths::{ensure_private_dir, MyrletHome, PathError};
use myrlet_protocol::{ClaimTaskRequest, DeliveryId, ExecutorId, SlotId, WorkerId};
use myrlet_worker::{ExecutorAdapter, HandlingEnded, LocalRegistry, RegistryError};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{watch, Notify};

use crate::config::WorkerConfig;
use crate::credentials::WorkerCredentials;
use crate::reporting::HubReporter;
use crate::tls::{self, TlsError};
use crate::wss::WssError;

/// Progress and Result file poll interval.
const REPORT_POLL: Duration = Duration::from_millis(500);
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Owns one Worker's configuration, credentials, adapters, and reporting tasks.
///
/// The handle is cloneable so an observer (tests, or a future local status surface) can
/// inspect the same runtime the run loop owns without a second copy.
#[derive(Clone)]
pub struct WorkerRuntime {
    core: Arc<RuntimeCore>,
}

impl WorkerRuntime {
    /// Pending-claim retry interval used while a lost acknowledgement is unexplained.
    pub const CLAIM_RETRY_INTERVAL: Duration = Duration::from_secs(1);
}

/// The live, reloadable half of the runtime: current config, reported inventory, adapters.
///
/// Claim admission reads this one snapshot, so the Slot selected for a claim and the
/// adapters used to admit it always come from the same configuration. Activation swaps
/// all four fields under one write guard, and a pending report pauses admission in the
/// same commit.
pub(super) struct RuntimeState {
    pub(super) config: WorkerConfig,
    pub(super) inventory: WorkerInventory,
    pub(super) adapters: HashMap<(SlotId, ExecutorId), Arc<dyn ExecutorAdapter>>,
    /// A locally reloaded inventory still awaits a Hub acknowledgement. While set, no
    /// new claim is started; a report already in flight still completes.
    pub(super) inventory_pending: bool,
}

/// Adapters pinned when a Claim is sent, before a possible reload.
pub(super) struct ClaimAdapters {
    pub(super) default: Option<ExecutorId>,
    pub(super) adapters: HashMap<ExecutorId, Arc<dyn ExecutorAdapter>>,
}

impl ClaimAdapters {
    pub(super) fn executor_id(&self) -> Option<ExecutorId> {
        self.default.clone()
    }

    pub(super) fn get(&self, executor_id: &ExecutorId) -> Option<Arc<dyn ExecutorAdapter>> {
        self.adapters.get(executor_id).cloned()
    }
}

pub(super) struct RuntimeCore {
    pub(super) home: MyrletHome,
    pub(super) state: RwLock<RuntimeState>,
    pub(super) creds: WorkerCredentials,
    pub(super) registry: Arc<LocalRegistry>,
    pub(super) reporter: Arc<HubReporter>,
    pub(super) tls: Arc<rustls::ClientConfig>,
    /// Current non-zero `connectionEpoch`; `0` marks "no live session".
    epoch: AtomicU64,
    reconnect: Notify,
    /// Signals the session to send the current inventory after a local reload.
    pub(super) inventory_changed: Notify,
    shutting_down: AtomicBool,
    /// Serializes local registry read-modify-write cycles.
    gate: Mutex<()>,
    /// One async lock per Delivery, so a poll, exit, resync, or manual sync of the same
    /// Delivery never overtakes another. In memory only; a restart re-reads the Registry.
    reconciliation: Mutex<HashMap<DeliveryId, Arc<tokio::sync::Mutex<()>>>>,
    /// The adapter that admitted each committed Delivery, so a config swap never strands
    /// an execution's cancellation or reporting path.
    pub(super) delivery_adapters: Mutex<HashMap<DeliveryId, Arc<dyn ExecutorAdapter>>>,
    /// The session retries its single outstanding claim after an ambiguous response.
    pending_claim: Mutex<Option<ClaimTaskRequest>>,
    /// A claim HTTP request is outstanding, even though the session selected its Slot.
    pub(super) claim_in_flight: AtomicBool,
    /// The adapter selected with the outstanding claim, retained across a config reload.
    pub(super) pending_adapters: Mutex<Option<ClaimAdapters>>,
    on_exit: UnboundedSender<HandlingEnded>,
    on_exit_rx: Mutex<Option<UnboundedReceiver<HandlingEnded>>>,
}

impl WorkerRuntime {
    /// Prepare managed directories, the local registry, adapters, and verified transport.
    pub fn new(
        home: MyrletHome,
        config: WorkerConfig,
        credentials: WorkerCredentials,
    ) -> Result<Self, RuntimeError> {
        ensure_home(&home)?;
        let registry = LocalRegistry::open(home.worker_state_dir())?;
        let revision = registry.load_or_bump_inventory_revision(&config.inventory_fingerprint())?;
        let inventory = config.build_inventory(revision, crate::now_ms());
        let adapters = config.build_adapters();
        let ca_file = tls::ca_file_from_env();
        let reporter = HubReporter::new(
            config.hub_url.clone(),
            credentials.credential.clone(),
            tls::build_http_client(ca_file.as_deref())?,
        );
        let tls_config = tls::build_tls_config(ca_file.as_deref())?;
        let (on_exit, on_exit_rx) = mpsc::unbounded_channel();
        Ok(Self {
            core: Arc::new(RuntimeCore {
                home,
                state: RwLock::new(RuntimeState {
                    config,
                    inventory,
                    adapters,
                    inventory_pending: false,
                }),
                creds: credentials,
                registry: Arc::new(registry),
                reporter: Arc::new(reporter),
                tls: tls_config,
                epoch: AtomicU64::new(0),
                reconnect: Notify::new(),
                inventory_changed: Notify::new(),
                shutting_down: AtomicBool::new(false),
                gate: Mutex::new(()),
                reconciliation: Mutex::new(HashMap::new()),
                delivery_adapters: Mutex::new(HashMap::new()),
                pending_claim: Mutex::new(None),
                claim_in_flight: AtomicBool::new(false),
                pending_adapters: Mutex::new(None),
                on_exit,
                on_exit_rx: Mutex::new(Some(on_exit_rx)),
            }),
        })
    }

    /// Run until `shutdown` is signalled: reconnect on loss, drain owned work on exit.
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<(), RuntimeError> {
        ensure_home(&self.core.home)?;
        let retained = self.core.registry.load_all()?;
        tracing::info!(retained = retained.len(), "worker runtime starting");
        let (exit_task, report_task) = Arc::clone(&self.core).spawn_reports();
        let socket_path = self.core.home.worker_control_socket();
        let control = tokio::spawn({
            let core = Arc::clone(&self.core);
            async move { local::serve(core, &socket_path).await }
        });
        let watcher = tokio::spawn({
            let core = Arc::clone(&self.core);
            async move { core.watch_config().await }
        });
        let result = self.core.connect_loop(&mut shutdown).await;
        if result.is_ok() {
            self.core.drain().await;
        }
        watcher.abort();
        control.abort();
        exit_task.abort();
        report_task.abort();
        result
    }

    /// Operator-configured graceful shutdown drain bound.
    pub fn shutdown_timeout_ms(&self) -> u64 {
        self.core.shutdown_timeout_ms()
    }

    /// Number of adapters still pinned to committed Deliveries (observability + tests).
    #[doc(hidden)]
    pub fn pinned_adapter_count(&self) -> usize {
        self.core
            .delivery_adapters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Number of adapters pinned with the outstanding claim, if one is unresolved.
    #[doc(hidden)]
    pub fn pending_adapter_count(&self) -> usize {
        self.core
            .pending_adapters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map_or(0, |adapters| adapters.adapters.len())
    }
}

fn ensure_home(home: &MyrletHome) -> Result<(), RuntimeError> {
    ensure_private_dir(&home.worker_dir())?;
    ensure_private_dir(&home.worker_state_dir())?;
    ensure_private_dir(&home.worker_progress_dir())?;
    ensure_private_dir(&home.worker_results_dir())?;
    Ok(())
}

/// Resolve when the process receives `SIGTERM` or `SIGINT`.
pub async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut terminate), Ok(mut interrupt)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            return;
        };
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

impl RuntimeCore {
    /// The live config/inventory/adapters. Callers never hold this guard across an await.
    pub(super) fn state(&self) -> std::sync::RwLockReadGuard<'_, RuntimeState> {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn inventory(&self) -> WorkerInventory {
        self.state().inventory.clone()
    }

    pub(super) fn hub_url(&self) -> String {
        self.state().config.hub_url.clone()
    }

    pub(super) fn shutdown_timeout_ms(&self) -> u64 {
        self.state().config.shutdown_timeout_ms()
    }

    /// The live connection epoch, or `0` when no session is established.
    pub(super) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// Connect, serve one session, and back off before reconnecting.
    async fn connect_loop(&self, shutdown: &mut watch::Receiver<bool>) -> Result<(), RuntimeError> {
        let mut backoff = INITIAL_BACKOFF;
        loop {
            if *shutdown.borrow() {
                return Ok(());
            }
            match self.session(shutdown).await {
                Ok(session::Outcome::Shutdown) => return Ok(()),
                Ok(session::Outcome::Lost) => backoff = INITIAL_BACKOFF,
                Err(error) => tracing::warn!(%error, "worker session ended with an error"),
            }
            self.epoch.store(0, Ordering::SeqCst);
            let closed = tokio::select! {
                () = tokio::time::sleep(backoff) => {
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    false
                }
                changed = shutdown.changed() => changed.is_err(),
            };
            if closed || *shutdown.borrow() {
                return Ok(());
            }
        }
    }
}

/// Runtime failures that end a connection attempt or the whole Worker.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error(transparent)]
    Wss(#[from] WssError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Report(#[from] crate::reporting::ReportError),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error("hub.welcome named worker {reported} but this Worker is {expected}")]
    WorkerMismatch {
        reported: WorkerId,
        expected: WorkerId,
    },
    #[error("hub.welcome issued a zero connection epoch")]
    ZeroEpoch,
    #[error("the Hub closed the connection before hub.welcome")]
    NoWelcome,
}

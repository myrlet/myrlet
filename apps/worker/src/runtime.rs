//! Worker daemon runtime: outbound WSS session, local Delivery registry, HTTP reporting.
//!
//! Invariants: a claimed Delivery is registered durably **before** the Harness starts, a
//! replayed claim reply never runs twice, a post-registration submit failure stays
//! unreported, no terminal verdict is ever fabricated, and no Harness-owned file is ever
//! written here.

mod deliveries;
mod local;
mod reports;
mod session;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use myrlet_protocol::inventory::WorkerInventory;
use myrlet_protocol::paths::{ensure_private_dir, MyrletHome, PathError};
use myrlet_protocol::{ClaimTaskRequest, DeliveryId, ExecutorId, SlotId, WorkerId};
use myrlet_worker::{ExecutorAdapter, HandlingEnded, LocalRegistry, RegistryError};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;

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
pub struct WorkerRuntime {
    core: Arc<RuntimeCore>,
}

impl WorkerRuntime {
    /// Pending-claim retry interval used while a lost acknowledgement is unexplained.
    pub const CLAIM_RETRY_INTERVAL: Duration = Duration::from_secs(1);
}

struct RuntimeCore {
    home: MyrletHome,
    config: WorkerConfig,
    creds: WorkerCredentials,
    inventory: WorkerInventory,
    adapters: HashMap<(SlotId, ExecutorId), Arc<dyn ExecutorAdapter>>,
    registry: Arc<LocalRegistry>,
    reporter: Arc<HubReporter>,
    tls: Arc<rustls::ClientConfig>,
    /// Current non-zero `connectionEpoch`; `0` marks "no live session".
    epoch: AtomicU64,
    shutting_down: AtomicBool,
    /// Serializes local registry read-modify-write cycles.
    gate: Mutex<()>,
    /// One async lock per Delivery, so a poll, exit, resync, or manual sync of the same
    /// Delivery never overtakes another. In memory only; a restart re-reads the Registry.
    reconciliation: Mutex<HashMap<DeliveryId, Arc<tokio::sync::Mutex<()>>>>,
    /// The session retries its single outstanding claim after an ambiguous response.
    pending_claim: Mutex<Option<ClaimTaskRequest>>,
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
                config,
                creds: credentials,
                inventory,
                adapters,
                registry: Arc::new(registry),
                reporter: Arc::new(reporter),
                tls: tls_config,
                epoch: AtomicU64::new(0),
                shutting_down: AtomicBool::new(false),
                gate: Mutex::new(()),
                reconciliation: Mutex::new(HashMap::new()),
                pending_claim: Mutex::new(None),
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
        let result = self.core.connect_loop(&mut shutdown).await;
        if result.is_ok() {
            self.core.drain().await;
        }
        control.abort();
        exit_task.abort();
        report_task.abort();
        result
    }

    /// Operator-configured graceful shutdown drain bound.
    pub fn shutdown_timeout_ms(&self) -> u64 {
        self.core.config.shutdown_timeout_ms()
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

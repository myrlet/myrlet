//! Worker daemon runtime: outbound WSS session, local Delivery registry, HTTP reporting.
//!
//! Invariants: a Delivery is registered durably **before** it is submitted, replayed
//! offers never double-submit, a post-registration submit failure stays unreported, no
//! terminal verdict is ever fabricated, and the Progress file is never written here.

mod deliveries;
mod reports;
mod session;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use myrlet_protocol::inventory::WorkerInventory;
use myrlet_protocol::paths::{ensure_private_dir, MyrletHome, PathError};
use myrlet_protocol::{DeliveryId, ExecutorId, SlotId, WorkerId};
use myrlet_worker::{ExecutorAdapter, HandlingEnded, LocalRegistry, RegistryError};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;

use crate::config::WorkerConfig;
use crate::credentials::WorkerCredentials;
use crate::reporting::HubReporter;
use crate::tls::{self, TlsError};
use crate::wss::WssError;

/// Progress file poll interval.
const PROGRESS_POLL: Duration = Duration::from_millis(500);
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Owns one Worker's configuration, credentials, adapters, and reporting tasks.
pub struct WorkerRuntime {
    core: Arc<RuntimeCore>,
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
    /// Last text hash relayed per Delivery, so unchanged Progress is not re-posted.
    progress: Mutex<HashMap<DeliveryId, [u8; 32]>>,
    shutting_down: AtomicBool,
    /// Serializes read-modify-write cycles on local Delivery records.
    gate: Mutex<()>,
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
                progress: Mutex::new(HashMap::new()),
                shutting_down: AtomicBool::new(false),
                gate: Mutex::new(()),
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
        let (exit_task, progress_task) = Arc::clone(&self.core).spawn_reports();
        let result = self.core.connect_loop(&mut shutdown).await;
        if result.is_ok() {
            self.core.drain().await;
        }
        exit_task.abort();
        progress_task.abort();
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

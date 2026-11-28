//! Myrlet Hub: authenticated HTTP/SSE control plane and the Worker WSS server.

pub mod auth;
pub mod config;
pub mod error;
pub mod http;
pub mod state;
pub mod wakeup;
pub mod ws;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use myrlet_store::HubStore;
use tokio::net::TcpListener;

pub use config::HubConfig;
pub use error::ConfigError;
pub use http::build_router;
pub use state::HubState;

/// Hub startup or serving failure.
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Store(#[from] myrlet_store::StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Open the Hub store and serve until `SIGTERM` / `SIGINT` completes a bounded drain.
pub async fn serve(config: HubConfig) -> Result<(), HubError> {
    let hub_id = config::load_or_init_hub_id(&config.home)?;
    let store = HubStore::open(&config.home.hub_database())?;
    let state = HubState::new(config.clone(), Arc::new(store), hub_id);
    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(bind = %config.bind_addr, hub_id = %state.hub_id, "myrlet hub listening");
    wakeup::spawn_wakeup_loop(&state);
    axum::serve(listener, build_router(state.clone()))
        .with_graceful_shutdown(shutdown_signal(state))
        .await?;
    Ok(())
}

async fn shutdown_signal(state: HubState) {
    wait_for_signal().await;
    tracing::info!("shutdown signal received; draining in-flight commands");
    state.shutting_down.store(true, Ordering::SeqCst);
    let timeout = Duration::from_millis(state.config.shutdown_timeout_ms);
    let deadline = tokio::time::Instant::now() + timeout;
    while state.command_permits.available_permits() < state.config.max_concurrent_requests
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for worker_id in state.online_map().into_keys().collect::<Vec<_>>() {
        state.disconnect_worker(&worker_id);
    }
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = terminate.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
                return;
            }
            Err(error) => tracing::warn!(%error, "SIGTERM listener unavailable"),
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

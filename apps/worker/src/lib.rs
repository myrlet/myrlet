//! Myrlet Worker daemon library: configuration, credentials, verified TLS, WSS transport,
//! Hub HTTP reporting, and the runtime that ties them together.

pub mod config;
pub mod credentials;
pub mod reporting;
pub mod runtime;
pub mod tls;
pub mod wss;

pub use config::{ConfigError, ExecutorConfig, SlotConfig, WorkerConfig};
pub use credentials::{CredentialsError, WorkerCredentials};
pub use reporting::{HubReporter, ReportError};
pub use runtime::{wait_for_shutdown_signal, RuntimeError, WorkerRuntime};
pub use tls::{build_http_client, build_tls_config, hub_ws_url, TlsError};
pub use wss::WssError;

/// Wall-clock milliseconds since the Unix epoch (saturating at `0` before it).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

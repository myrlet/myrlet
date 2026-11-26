//! Myrlet Hub daemon (HTTP/SSE control plane and Worker WSS server).

use std::process::ExitCode;

use myrlet_hub::{serve, HubConfig};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let config = match HubConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "invalid hub configuration");
            return ExitCode::FAILURE;
        }
    };
    match serve(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "hub stopped with an error");
            ExitCode::FAILURE
        }
    }
}

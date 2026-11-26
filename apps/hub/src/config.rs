//! Hub configuration: `MYRLET_HOME`, optional `hub/config.toml`, env overrides, and `hubId`.

use std::net::SocketAddr;

use myrlet_protocol::{verify_private_file, write_private_file_atomic, MyrletHome};
use serde::Deserialize;

use crate::error::ConfigError;

const BIND_ADDR: &str = "127.0.0.1:4140";
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_BUNDLE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT_MS: u64 = 30_000;
const MAX_CONCURRENT: usize = 128;
const SHUTDOWN_TIMEOUT_MS: u64 = 30_000;
const EVENT_RETENTION: usize = 10_000;
const SNAPSHOT_MAX_TASKS: usize = 500;
const HEARTBEAT_INTERVAL_MS: u64 = 5_000;
const LEASE_TTL_MS: u64 = 15_000;

const ENV_ADMIN_TOKEN: &str = "MYRLET_ADMIN_TOKEN";
const ENV_BIND: &str = "MYRLET_HUB_BIND";
const ENV_SHUTDOWN_TIMEOUT_MS: &str = "MYRLET_HUB_SHUTDOWN_TIMEOUT_MS";

#[derive(Clone)]
pub struct HubConfig {
    pub bind_addr: SocketAddr,
    pub admin_token: String,
    pub home: MyrletHome,
    pub max_body_bytes: usize,
    pub max_bundle_bytes: usize,
    pub request_timeout_ms: u64,
    pub max_concurrent_requests: usize,
    pub shutdown_timeout_ms: u64,
    pub event_retention_window: usize,
    pub snapshot_max_tasks: usize,
    pub heartbeat_interval_ms: u64,
    pub lease_ttl_ms: u64,
}

impl std::fmt::Debug for HubConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubConfig")
            .field("bind_addr", &self.bind_addr)
            .field("admin_token", &"<redacted>")
            .field("home", &self.home)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("max_bundle_bytes", &self.max_bundle_bytes)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field("max_concurrent_requests", &self.max_concurrent_requests)
            .field("shutdown_timeout_ms", &self.shutdown_timeout_ms)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileConfig {
    bind_addr: Option<String>,
    max_body_bytes: Option<usize>,
    max_bundle_bytes: Option<usize>,
    request_timeout_ms: Option<u64>,
    max_concurrent_requests: Option<usize>,
    shutdown_timeout_ms: Option<u64>,
    event_retention_window: Option<usize>,
    snapshot_max_tasks: Option<usize>,
    heartbeat_interval_ms: Option<u64>,
    lease_ttl_ms: Option<u64>,
}

impl HubConfig {
    /// Resolve the environment and the optional private `hub/config.toml` (env wins).
    pub fn from_env() -> Result<Self, ConfigError> {
        let home = MyrletHome::resolve(None)?;
        let file = read_file_config(&home)?;
        let bind_addr = match env_var(ENV_BIND).or(file.bind_addr) {
            Some(raw) => parse_addr(&raw)?,
            None => parse_addr(BIND_ADDR)?,
        };
        let shutdown_timeout_ms = match env_var(ENV_SHUTDOWN_TIMEOUT_MS) {
            Some(raw) => parse_u64(ENV_SHUTDOWN_TIMEOUT_MS, &raw)?,
            None => file.shutdown_timeout_ms.unwrap_or(SHUTDOWN_TIMEOUT_MS),
        };
        Ok(Self {
            bind_addr,
            admin_token: env_var(ENV_ADMIN_TOKEN).ok_or(ConfigError::MissingAdminToken)?,
            home,
            max_body_bytes: file.max_body_bytes.unwrap_or(MAX_BODY_BYTES),
            max_bundle_bytes: file.max_bundle_bytes.unwrap_or(MAX_BUNDLE_BYTES),
            request_timeout_ms: file.request_timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS),
            max_concurrent_requests: file.max_concurrent_requests.unwrap_or(MAX_CONCURRENT),
            shutdown_timeout_ms,
            event_retention_window: file.event_retention_window.unwrap_or(EVENT_RETENTION),
            snapshot_max_tasks: file.snapshot_max_tasks.unwrap_or(SNAPSHOT_MAX_TASKS),
            heartbeat_interval_ms: file.heartbeat_interval_ms.unwrap_or(HEARTBEAT_INTERVAL_MS),
            lease_ttl_ms: file.lease_ttl_ms.unwrap_or(LEASE_TTL_MS),
        })
    }
}

fn parse_addr(raw: &str) -> Result<SocketAddr, ConfigError> {
    raw.parse()
        .map_err(|_| ConfigError::InvalidBindAddr(raw.to_string()))
}

fn parse_u64(name: &str, raw: &str) -> Result<u64, ConfigError> {
    raw.parse().map_err(|_| ConfigError::InvalidNumber {
        name: name.to_string(),
        value: raw.to_string(),
    })
}

fn read_file_config(home: &MyrletHome) -> Result<FileConfig, ConfigError> {
    let path = home.hub_config();
    verify_private_file(&path)?;
    match std::fs::read(&path) {
        Ok(bytes) => toml::from_str(&String::from_utf8_lossy(&bytes)).map_err(ConfigError::Toml),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FileConfig::default()),
        Err(source) => Err(ConfigError::Io(source)),
    }
}

fn env_var(name: &str) -> Option<String> {
    match std::env::var_os(name) {
        Some(value) if !value.is_empty() => Some(value.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// `hub/credentials.json` document holding the installation ID.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HubCredentials {
    hub_id: String,
}

/// Read the private installation ID, creating `hub_<uuid>` atomically when absent.
pub fn load_or_init_hub_id(home: &MyrletHome) -> Result<String, ConfigError> {
    let path = home.hub_credentials();
    verify_private_file(&path)?;
    match std::fs::read(&path) {
        Ok(bytes) => {
            let credentials: HubCredentials =
                serde_json::from_slice(&bytes).map_err(ConfigError::Credentials)?;
            let suffix = credentials.hub_id.strip_prefix("hub_").unwrap_or_default();
            let canonical = !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
            if !canonical {
                return Err(ConfigError::InvalidHubId(credentials.hub_id));
            }
            Ok(credentials.hub_id)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let hub_id = format!("hub_{}", uuid::Uuid::new_v4().simple());
            let document = serde_json::to_vec(&serde_json::json!({ "hubId": hub_id }))
                .map_err(ConfigError::Credentials)?;
            write_private_file_atomic(&path, &document)?;
            verify_private_file(&path)?;
            Ok(hub_id)
        }
        Err(source) => Err(ConfigError::Io(source)),
    }
}

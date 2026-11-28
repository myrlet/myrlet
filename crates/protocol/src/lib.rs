//! Canonical Myrlet V1 domain types, HTTP/WSS wire DTOs, and runtime path safety.

pub mod api;
pub mod bundle;
pub mod delivery;
pub mod id;
pub mod inventory;
pub mod local;
pub mod paths;
pub mod progress;
pub mod result;
pub mod snapshot;
pub mod task;
pub mod ws;

/// Protocol version identifier for the Myrlet Worker WSS channel.
pub const WORKER_PROTOCOL_VERSION: &str = "myrlet.worker/1";

/// Schema version identifier for `GET /v1/snapshot`.
pub const SNAPSHOT_SCHEMA_VERSION: &str = "myrlet.snapshot/v1";

pub use api::*;
pub use bundle::*;
pub use delivery::*;
pub use id::*;
pub use inventory::*;
pub use local::*;
pub use paths::*;
pub use progress::*;
pub use result::*;
pub use snapshot::*;
pub use task::*;
pub use ws::*;

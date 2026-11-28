//! Worker local Delivery registry, Progress file watch, and thin Harness adapters.

pub mod adapter;
pub mod process_group;
pub mod progress_watch;
pub mod registry;
pub mod result_watch;

pub use adapter::harness::{HarnessCliAdapter, HarnessKind};
pub use adapter::process::ProcessAdapter;
pub use adapter::runner::ProcessRunner;
pub use adapter::{
    build_startup_prompt, AdapterError, ExecutorAdapter, HandlingEnded, SpawnSpec, SubmitRequest,
};
pub use myrlet_protocol::inventory::HostFacts;
pub use myrlet_protocol::WORKER_PROTOCOL_VERSION;
pub use progress_watch::{read_progress_if_changed, ObservedProgress};
pub use registry::{LocalDeliveryRecord, LocalRegistry, RegistryError};
pub use result_watch::{read_result, LocalResult, LocalResultError};

/// Basic Host facts only: OS, architecture, and whether the Worker runs in a container.
/// Worker Core never probes, installs, or manages toolchains or Harness binaries.
pub fn host_facts() -> HostFacts {
    use myrlet_protocol::inventory::DeploymentMode;
    HostFacts {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        deployment_mode: if std::path::Path::new("/.dockerenv").exists() {
            DeploymentMode::Container
        } else {
            DeploymentMode::Native
        },
    }
}

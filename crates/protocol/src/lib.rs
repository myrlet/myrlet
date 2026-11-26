//! Canonical Myrlet V1 domain types, wire DTOs, and runtime path safety.

/// Protocol version identifier for the Myrlet Worker WSS channel.
pub const WORKER_PROTOCOL_VERSION: &str = "myrlet.worker/1";

#[cfg(test)]
mod tests {
    use super::WORKER_PROTOCOL_VERSION;

    #[test]
    fn protocol_version_is_non_empty() {
        assert_eq!(WORKER_PROTOCOL_VERSION, "myrlet.worker/1");
    }
}

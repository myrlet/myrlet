//! Distinct Myrlet V1 identity types.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }

            pub fn is_valid_segment(&self) -> bool {
                !self.0.is_empty()
                    && self.0.len() <= 128
                    && self.0 != "."
                    && self.0 != ".."
                    && self
                        .0
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

define_id!(
    /// Durable logical unit of work owned by Hub.
    TaskId
);
define_id!(
    /// Durable identity of one Hub-to-Worker handoff.
    DeliveryId
);
define_id!(
    /// Enrolled machine identity (`wkr_` + 12 Crockford Base32 chars).
    WorkerId
);
define_id!(
    /// Selectable execution/concurrency unit inside a Worker.
    SlotId
);
define_id!(
    /// Operator-configured adapter selected for a Delivery.
    ExecutorId
);
define_id!(
    /// Opaque backend-local handle to one submitted execution.
    ExecutionRef
);

const CROCKFORD_BASE32: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl WorkerId {
    /// Generate a random Worker ID formatted as `wkr_` + 12 Crockford Base32 chars (60 bits).
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let digest = Sha256::digest(seed);
        let mut bits: u64 = 0;
        for byte in &digest[..8] {
            bits = (bits << 8) | u64::from(*byte);
        }
        bits >>= 4; // keep 60 bits

        let mut out = String::with_capacity(16);
        out.push_str("wkr_");
        for shift in (0..12).rev() {
            let idx = ((bits >> (shift * 5)) & 0x1f) as usize;
            out.push(CROCKFORD_BASE32[idx] as char);
        }
        Self(out)
    }

    /// Check canonical `wkr_` + 12 Crockford Base32 format.
    pub fn is_canonical(&self) -> bool {
        let Some(suffix) = self.0.strip_prefix("wkr_") else {
            return false;
        };
        suffix.len() == 12 && suffix.bytes().all(|b| CROCKFORD_BASE32.contains(&b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_worker_id_is_canonical() {
        let id = WorkerId::generate();
        assert!(id.is_canonical(), "unexpected worker id: {id}");
        assert!(id.is_valid_segment());
    }

    #[test]
    fn id_segment_validation_rejects_traversal() {
        assert!(!TaskId::new("..").is_valid_segment());
        assert!(!TaskId::new("a/b").is_valid_segment());
        assert!(TaskId::new("task_01-a.b").is_valid_segment());
    }
}

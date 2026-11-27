//! Optional latest-value Markdown Task result, owned by the Delivery's Worker.
//!
//! The Harness writes one local Markdown file; the Worker reconciles the Hub copy with
//! its current content. There is no artifact, attachment, history, or Result verifier
//! model (`task-model.md`), and no revision counter: the Hub keeps one latest value per
//! Delivery plus the SHA-256 of that value.

use crate::id::{DeliveryId, TaskId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Maximum accepted result text length in bytes (64 KiB).
pub const MAX_RESULT_BYTES: usize = 64 * 1024;

/// Lowercase hex SHA-256 of the exact UTF-8 result bytes.
///
/// The digest covers the Markdown text only; links inside it are text, and there is no
/// separate reference or attachment model to hash.
pub fn result_sha256(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Worker HTTP result reconciliation; the epoch fences it to the current connection.
///
/// `text: Some(..)` replaces the Hub copy. `text: None` states that, after local
/// recovery, there is no Result at all, and clears any stored copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResultReportRequest {
    pub epoch: u64,
    pub text: Option<String>,
}

impl ResultReportRequest {
    /// Validate bounds that do not depend on stored Delivery state.
    pub fn validate(&self) -> Result<(), ResultValidationError> {
        if self.epoch == 0 {
            return Err(ResultValidationError::ZeroEpoch);
        }
        if let Some(text) = &self.text {
            if text.len() > MAX_RESULT_BYTES {
                return Err(ResultValidationError::TooLarge);
            }
            if text.trim().is_empty() {
                return Err(ResultValidationError::Empty);
            }
        }
        Ok(())
    }

    /// Digest the value this report asks the Hub to hold, if any.
    pub fn digest(&self) -> Option<String> {
        self.text.as_deref().map(result_sha256)
    }
}

/// Acknowledgement of one reconciled result value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultReceipt {
    pub delivery_id: DeliveryId,
    pub task_id: TaskId,
    /// When the Hub last stored a value; `None` means the Delivery currently has no Result.
    pub recorded_at_ms: Option<i64>,
    /// SHA-256 of the stored value, so a client can compare without re-reading the text.
    pub result_sha256: Option<String>,
    /// This report removed a previously stored value.
    pub cleared: bool,
    /// The Hub already held exactly this value; nothing was written.
    pub duplicate: bool,
}

#[derive(Debug, Error)]
pub enum ResultValidationError {
    #[error("result exceeds the maximum size")]
    TooLarge,
    #[error("result is empty")]
    Empty,
    #[error("connection epoch must be non-zero")]
    ZeroEpoch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ResultValidationError as E;

    fn request(epoch: u64, text: Option<&str>) -> ResultReportRequest {
        ResultReportRequest {
            epoch,
            text: text.map(str::to_owned),
        }
    }

    #[test]
    fn request_rejects_zero_epoch_empty_and_oversized_text() {
        assert!(matches!(
            request(0, Some("x")).validate(),
            Err(E::ZeroEpoch)
        ));
        assert!(matches!(request(1, Some("  \n")).validate(), Err(E::Empty)));
        assert!(matches!(
            request(1, Some(&"x".repeat(MAX_RESULT_BYTES + 1))).validate(),
            Err(E::TooLarge)
        ));
        assert!(request(7, None).validate().is_ok());
    }

    #[test]
    fn clearing_and_content_share_one_wire_shape() {
        assert_eq!(
            serde_json::to_value(request(7, Some("text"))).unwrap(),
            serde_json::json!({ "epoch": 7, "text": "text" })
        );
        assert_eq!(
            serde_json::to_value(request(7, None)).unwrap(),
            serde_json::json!({ "epoch": 7, "text": null })
        );
        assert_eq!(request(7, None).digest(), None);
    }

    #[test]
    fn digest_is_stable_hex_sha256_of_the_text() {
        assert_eq!(request(1, Some("")).digest(), request(1, Some("")).digest());
        let hex = result_sha256("# Findings\n");
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(hex, result_sha256("# Findings\n\n- one"));
    }
}

//! Optional bounded Markdown Task result: text only, immutable, per Delivery.
//!
//! The Harness writes one local Markdown file; the Worker syncs its final text once.
//! There is no artifact, attachment, or Result verifier model (`task-model.md`).

use crate::id::{DeliveryId, TaskId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum accepted result text length in bytes (64 KiB).
pub const MAX_RESULT_BYTES: usize = 64 * 1024;

/// Worker HTTP result report; the epoch fences it to the current connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResultReportRequest {
    pub epoch: u64,
    pub text: String,
}

impl ResultReportRequest {
    /// Validate bounds that do not depend on stored Delivery state.
    pub fn validate(&self) -> Result<(), ResultValidationError> {
        if self.epoch == 0 {
            return Err(ResultValidationError::ZeroEpoch);
        }
        if self.text.len() > MAX_RESULT_BYTES {
            return Err(ResultValidationError::TooLarge);
        }
        if self.text.trim().is_empty() {
            return Err(ResultValidationError::Empty);
        }
        Ok(())
    }
}

/// Acknowledgement of a stored result; `duplicate` marks an identical replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultReceipt {
    pub delivery_id: DeliveryId,
    pub task_id: TaskId,
    pub recorded_at_ms: i64,
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

    #[test]
    fn request_rejects_zero_epoch_empty_and_oversized_text() {
        let request = |epoch: u64, text: &str| ResultReportRequest {
            epoch,
            text: text.to_owned(),
        };
        assert!(matches!(request(0, "x").validate(), Err(E::ZeroEpoch)));
        assert!(matches!(request(1, "  \n").validate(), Err(E::Empty)));
        assert!(matches!(
            request(1, &"x".repeat(MAX_RESULT_BYTES + 1)).validate(),
            Err(E::TooLarge)
        ));
        assert_eq!(
            serde_json::to_value(request(7, "text")).unwrap(),
            serde_json::json!({ "epoch": 7, "text": "text" })
        );
    }
}

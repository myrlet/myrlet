//! Progress and Finish reporting: bounded latest-value Progress, never a Result model.

use crate::id::{DeliveryId, TaskId};
use crate::task::TaskStatus;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum Harness-authored Progress file size in bytes (64 KiB).
pub const MAX_PROGRESS_FILE_BYTES: usize = 64 * 1024;
/// Maximum Progress text length in bytes (16 KiB).
pub const MAX_PROGRESS_TEXT_BYTES: usize = 16 * 1024;

/// Harness-authored Progress file document. Unknown fields are ignored by design.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressFileDocument {
    pub text: String,
}

impl ProgressFileDocument {
    /// Parse a Progress file with the file and text size bounds applied.
    pub fn parse_bounded(bytes: &[u8]) -> Result<Self, ProgressValidationError> {
        if bytes.len() > MAX_PROGRESS_FILE_BYTES {
            return Err(ProgressValidationError::FileTooLarge);
        }
        let document: Self =
            serde_json::from_slice(bytes).map_err(ProgressValidationError::InvalidJson)?;
        if document.text.len() > MAX_PROGRESS_TEXT_BYTES {
            return Err(ProgressValidationError::TextTooLarge);
        }
        Ok(document)
    }
}

/// Worker HTTP Progress report; the epoch fences it to the current connection.
///
/// The Worker is the Delivery's single Progress writer: each accepted report replaces the
/// Hub's one stored value. There is no Progress history and no separate clear operation,
/// because a stored Progress row is execution-start evidence that never becomes absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgressReportRequest {
    pub epoch: u64,
    pub observed_at_ms: i64,
    pub text: String,
}

impl ProgressReportRequest {
    /// Validate bounds that do not depend on stored Delivery state.
    pub fn validate(&self) -> Result<(), ProgressValidationError> {
        if self.epoch == 0 {
            return Err(ProgressValidationError::ZeroEpoch);
        }
        if self.text.len() > MAX_PROGRESS_TEXT_BYTES {
            return Err(ProgressValidationError::TextTooLarge);
        }
        Ok(())
    }
}

/// Acknowledgement of an accepted Progress report; `duplicate` marks an equal replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressReceipt {
    pub delivery_id: DeliveryId,
    pub accepted_at_ms: i64,
    /// Immutable timestamp of the first accepted Progress (execution-start evidence).
    pub first_accepted_at_ms: i64,
    pub duplicate: bool,
}

/// Worker HTTP Finish report. Outcome-free: no success/failure flag or summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinishReportRequest {
    pub epoch: u64,
}

/// Acknowledgement of a Finish report; `changed: false` marks an idempotent replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishReceipt {
    pub delivery_id: DeliveryId,
    pub task_id: TaskId,
    pub task_status: TaskStatus,
    pub changed: bool,
}

/// Latest accepted Progress for a Delivery; Hub stores no Progress history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryProgressView {
    pub delivery_id: DeliveryId,
    pub task_id: TaskId,
    pub text: String,
    pub observed_at_ms: i64,
    pub received_at_ms: i64,
    /// Immutable timestamp of the first accepted Progress for this Delivery.
    pub first_received_at_ms: i64,
}

impl DeliveryProgressView {
    /// A stored Progress row only exists after the Harness started, so it is start evidence.
    pub fn execution_started(&self) -> bool {
        true
    }
}

#[derive(Debug, Error)]
pub enum ProgressValidationError {
    #[error("progress file exceeds the maximum size")]
    FileTooLarge,
    #[error("progress file is not a valid Progress document")]
    InvalidJson(#[source] serde_json::Error),
    #[error("progress text exceeds the maximum size")]
    TextTooLarge,
    #[error("connection epoch must be non-zero")]
    ZeroEpoch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProgressValidationError as E;

    #[test]
    fn document_ignores_unknown_fields_and_enforces_bounds() {
        let document = ProgressFileDocument::parse_bounded(br#"{"text":"ok","extra":1}"#).unwrap();
        assert_eq!(document.text, "ok");
        let text = "x".repeat(MAX_PROGRESS_TEXT_BYTES + 1);
        let large = format!(r#"{{"text":"{}"}}"#, text);
        assert!(ProgressFileDocument::parse_bounded(large.as_bytes()).is_err());
        assert!(ProgressFileDocument::parse_bounded(b"not json").is_err());
    }

    #[test]
    fn report_validates_bounds_camel_case_and_unknown_fields() {
        let mut report: ProgressReportRequest =
            serde_json::from_str(r#"{"epoch":0,"observedAtMs":1,"text":""}"#).unwrap();
        assert!(matches!(report.validate(), Err(E::ZeroEpoch)));
        report.epoch = 1;
        report.text = "x".repeat(MAX_PROGRESS_TEXT_BYTES + 1);
        assert!(matches!(report.validate(), Err(E::TextTooLarge)));
        let raw = r#"{"epoch":7,"observedAtMs":10,"text":"ok"}"#;
        let report: ProgressReportRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(report.observed_at_ms, 10);
        assert!(serde_json::from_str::<ProgressReportRequest>(
            r#"{"epoch":7,"observedAtMs":10,"text":"ok","x":1}"#
        )
        .is_err());
    }

    #[test]
    fn view_reports_execution_started() {
        let view: DeliveryProgressView = serde_json::from_value(serde_json::json!({
            "deliveryId": "d", "taskId": "t", "text": "x",
            "observedAtMs": 1, "receivedAtMs": 2, "firstReceivedAtMs": 2
        }))
        .unwrap();
        assert!(view.execution_started());
        assert_eq!(view.first_received_at_ms, 2);
    }
}

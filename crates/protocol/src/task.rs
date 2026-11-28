//! Durable Task domain types: five-state lifecycle, immutable definition, and Hub record.

use crate::id::{ExecutorId, TaskId, WorkerId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

pub const MAX_TITLE_CHARS: usize = 256;
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
pub const MAX_CATEGORY_CHARS: usize = 64;
pub const MAX_TAGS: usize = 64;
pub const MAX_TAG_CHARS: usize = 64;
pub const MAX_DEPENDENCIES: usize = 64;
pub const MAX_EXTERNAL_REFERENCES: usize = 32;
pub const MAX_REQUIRED_CAPABILITIES: usize = 32;

/// The five Hub-owned Task states; no `running` and no success/failure verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Held,
    Queued,
    Dispatched,
    Finished,
    Cancelled,
}

impl TaskStatus {
    /// `finished` and `cancelled` never change again.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Finished | Self::Cancelled)
    }

    /// Whether Hub may move a Task directly from `self` to `next`.
    ///
    /// Submission moves a Task straight to `queued` when its prerequisites are already
    /// `finished`, and leaves it `held` otherwise. A claim commits the Delivery and moves
    /// the Task to `dispatched`.
    pub fn can_transition_to(&self, next: TaskStatus) -> bool {
        use TaskStatus::*;
        if next == Cancelled {
            return !self.is_terminal();
        }
        matches!(
            (self, next),
            (Held, Queued) | (Queued, Dispatched) | (Dispatched, Finished)
        )
    }
}

/// Routing constraints; capabilities and Slot choice are hard, the rest are soft.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRouting {
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    pub preferred_worker_id: Option<WorkerId>,
    pub preferred_role: Option<String>,
    pub preferred_executor_id: Option<ExecutorId>,
}

/// Execution hints passed through to the Harness without Hub interpretation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskExecution {
    pub preferred_model: Option<String>,
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub parameters: serde_json::Map<String, serde_json::Value>,
}

/// Immutable client-supplied fields; Hub owns identity, status, and timestamps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskDefinition {
    pub client_ref: Option<String>,
    pub parent_task_id: Option<TaskId>,
    pub source_task_id: Option<TaskId>,
    #[serde(default)]
    pub dependencies: Vec<TaskId>,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub title: String,
    pub prompt: String,
    pub input: Option<serde_json::Value>,
    pub expected_output: Option<serde_json::Value>,
    #[serde(default)]
    pub external_references: Vec<String>,
    #[serde(default)]
    pub routing: TaskRouting,
    #[serde(default)]
    pub execution: TaskExecution,
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl TaskDefinition {
    /// Validate immutable client-supplied fields before Hub persists the Task.
    pub fn validate(&self) -> Result<(), TaskValidationError> {
        if self.title.trim().is_empty() {
            return Err(TaskValidationError::EmptyTitle);
        }
        if self.title.chars().count() > MAX_TITLE_CHARS {
            return Err(TaskValidationError::TitleTooLong);
        }
        if self.prompt.trim().is_empty() {
            return Err(TaskValidationError::EmptyPrompt);
        }
        if self.prompt.len() > MAX_PROMPT_BYTES {
            return Err(TaskValidationError::PromptTooLong);
        }
        if self.category.is_empty() {
            return Err(TaskValidationError::EmptyCategory);
        }
        if self.category.len() > MAX_CATEGORY_CHARS {
            return Err(TaskValidationError::CategoryTooLong);
        }
        if !self.category.bytes().all(is_category_byte) {
            return Err(TaskValidationError::InvalidCategory);
        }
        if self.tags.len() > MAX_TAGS {
            return Err(TaskValidationError::TooManyTags);
        }
        if self
            .tags
            .iter()
            .any(|tag| tag.chars().count() > MAX_TAG_CHARS)
        {
            return Err(TaskValidationError::TagTooLong);
        }
        if self.dependencies.len() > MAX_DEPENDENCIES {
            return Err(TaskValidationError::TooManyDependencies);
        }
        let mut seen = HashSet::with_capacity(self.dependencies.len());
        if self.dependencies.iter().any(|dep| !seen.insert(dep)) {
            return Err(TaskValidationError::DuplicateDependency);
        }
        if self.external_references.len() > MAX_EXTERNAL_REFERENCES {
            return Err(TaskValidationError::TooManyExternalReferences);
        }
        if self.routing.required_capabilities.len() > MAX_REQUIRED_CAPABILITIES {
            return Err(TaskValidationError::TooManyRequiredCapabilities);
        }
        Ok(())
    }

    /// The status submission gives this Task: `queued` only when every declared
    /// dependency is already `finished`, otherwise `held`.
    pub fn submitted_status(&self, dependencies_finished: bool) -> TaskStatus {
        if dependencies_finished {
            TaskStatus::Queued
        } else {
            TaskStatus::Held
        }
    }
}

fn is_category_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'+' | b'-')
}

fn default_category() -> String {
    "general".to_string()
}

/// Rejection reasons for an invalid [`TaskDefinition`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TaskValidationError {
    #[error("title must not be empty")]
    EmptyTitle,
    #[error("title exceeds the maximum length")]
    TitleTooLong,
    #[error("prompt must not be empty")]
    EmptyPrompt,
    #[error("prompt exceeds the maximum size")]
    PromptTooLong,
    #[error("category must not be empty")]
    EmptyCategory,
    #[error("category exceeds the maximum length")]
    CategoryTooLong,
    #[error("category contains unsupported characters")]
    InvalidCategory,
    #[error("too many tags")]
    TooManyTags,
    #[error("tag exceeds the maximum length")]
    TagTooLong,
    #[error("too many dependencies")]
    TooManyDependencies,
    #[error("dependencies must be unique")]
    DuplicateDependency,
    #[error("too many external references")]
    TooManyExternalReferences,
    #[error("too many required capabilities")]
    TooManyRequiredCapabilities,
}

/// Derived per-status counts for a Task subtree; never changes the Parent's status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtreeCounts {
    pub total: u64,
    pub held: u64,
    pub queued: u64,
    pub dispatched: u64,
    pub finished: u64,
    pub cancelled: u64,
}

/// One declared dependency that keeps a Task `held`, with its current status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockingDependency {
    pub task_id: TaskId,
    pub status: TaskStatus,
}

/// Authoritative Hub-owned Task record returned by Task reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: TaskId,
    pub root_task_id: TaskId,
    pub depth: u8,
    pub status: TaskStatus,
    pub version: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub task: TaskDefinition,
    pub current_delivery: Option<crate::delivery::DeliveryView>,
    /// Dependencies that are not `finished`, with their status, in Task-ID order.
    #[serde(default)]
    pub blocked_by: Vec<BlockingDependency>,
    /// Latest Harness-written Markdown result, when the Delivery has one.
    #[serde(default)]
    pub result_markdown: Option<String>,
    /// When Hub stored the current result; independent of `finished_at_ms`.
    #[serde(default)]
    pub result_recorded_at_ms: Option<i64>,
    pub subtree: SubtreeCounts,
}

/// Limited one-hop projection of a Task related to a Worker's assigned Task.
///
/// It carries only coordination identifiers, title, status, and hierarchy/dependency
/// relationships. A worker-tier read never receives the related Task's prompt, input,
/// metadata, Delivery, Progress, or Result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedTaskSummary {
    pub id: TaskId,
    pub parent_task_id: Option<TaskId>,
    pub depth: u8,
    #[serde(default)]
    pub dependencies: Vec<TaskId>,
    #[serde(default)]
    pub blocked_by: Vec<BlockingDependency>,
    pub title: String,
    pub status: TaskStatus,
    pub version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> TaskDefinition {
        let raw = r#"{"title":"Verify the build","prompt":"Run the test suite."}"#;
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn statuses_and_transitions_follow_the_contract() {
        let json = serde_json::to_string(&TaskStatus::Dispatched).unwrap();
        assert_eq!(json, "\"dispatched\"");
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Held.is_terminal());
        assert!(TaskStatus::Held.can_transition_to(TaskStatus::Queued));
        assert!(!TaskStatus::Held.can_transition_to(TaskStatus::Dispatched));
        assert!(TaskStatus::Queued.can_transition_to(TaskStatus::Dispatched));
        assert!(TaskStatus::Dispatched.can_transition_to(TaskStatus::Finished));
        assert!(TaskStatus::Dispatched.can_transition_to(TaskStatus::Cancelled));
        assert!(!TaskStatus::Finished.can_transition_to(TaskStatus::Cancelled));
        assert!(!TaskStatus::Cancelled.can_transition_to(TaskStatus::Queued));
    }

    #[test]
    fn submission_authorizes_only_ready_tasks() {
        let task = definition();
        assert_eq!(task.submitted_status(true), TaskStatus::Queued);
        assert_eq!(task.submitted_status(false), TaskStatus::Held);
    }

    #[test]
    fn definitions_default_validate_and_reject_bad_values() {
        let task = definition();
        assert_eq!(task.category, "general");
        assert!(task.validate().is_ok());
        let unknown =
            serde_json::from_str::<TaskDefinition>(r#"{"title":"t","prompt":"p","priority":7}"#);
        assert!(unknown.is_err());
        let mut task = definition();
        task.title = "  ".into();
        assert_eq!(task.validate(), Err(TaskValidationError::EmptyTitle));
        let mut task = definition();
        task.category = "not valid".into();
        assert_eq!(task.validate(), Err(TaskValidationError::InvalidCategory));
        let mut task = definition();
        task.dependencies = vec![TaskId::new("task_a"), TaskId::new("task_a")];
        let duplicate = task.validate();
        assert_eq!(duplicate, Err(TaskValidationError::DuplicateDependency));
    }
}

//! The strict two-method Harness Adapter contract: `submit` and `cancel` only.
//! Capability probing, validation, inspection, reconciliation, reattach, resume, and
//! interactive steering/input are deliberately absent (`worker-protocol.md`).

use async_trait::async_trait;
use myrlet_protocol::delivery::CancelState;
use myrlet_protocol::id::{DeliveryId, ExecutionRef, ExecutorId, TaskId};
use myrlet_protocol::task::TaskDefinition;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;

pub mod harness;
pub mod process;
pub mod runner;

/// One accepted Delivery handed to an Adapter, with its Progress and result file paths.
#[derive(Debug, Clone)]
pub struct SubmitRequest {
    pub task_id: TaskId,
    pub delivery_id: DeliveryId,
    pub task: TaskDefinition,
    pub progress_path: PathBuf,
    pub result_path: PathBuf,
}

/// Sent when the Adapter observes end of handling, so the Worker can post `/finish`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlingEnded {
    pub delivery_id: DeliveryId,
    pub execution_ref: ExecutionRef,
}

/// How to start one execution: the command line, optional stdin prompt, and time bound.
#[derive(Debug, Clone, Copy)]
pub struct SpawnSpec<'a> {
    pub program: &'a str,
    pub args: &'a [String],
    /// Startup prompt handed over stdin, or `None` when the prompt is an argument.
    pub stdin_prompt: Option<&'a str>,
    /// `None` lets the Harness decide when handling ends.
    pub timeout: Option<Duration>,
}

/// Local submission failure observed before any execution handle exists.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("failed to spawn executor {executor}: {source}")]
    Spawn {
        executor: String,
        #[source]
        source: std::io::Error,
    },
    #[error("executor {0} produced no controllable process handle")]
    UncontrollableProcess(String),
}

/// The generic Harness Adapter surface: submit one Delivery, cancel one execution.
#[async_trait]
pub trait ExecutorAdapter: Send + Sync {
    /// Configured Executor identity reported in inventory.
    fn id(&self) -> &ExecutorId;

    /// Adapter kind label reported in inventory (e.g. `claude`, `process`).
    fn kind(&self) -> &'static str;

    /// Start handling one Delivery. `on_exit` fires once when handling ends locally.
    async fn submit(
        &self,
        req: SubmitRequest,
        on_exit: UnboundedSender<HandlingEnded>,
    ) -> Result<ExecutionRef, AdapterError>;

    /// Signal the backend and relay its answer without claiming physical termination.
    async fn cancel(&self, execution_ref: &ExecutionRef) -> CancelState;
}

/// Minimal startup prompt: Task identification, the Progress and result file paths, and
/// the prompt. Everything else the Harness fetches itself through the `myrlet` CLI.
pub fn build_startup_prompt(req: &SubmitRequest) -> String {
    format!(
        "Myrlet Task ID: {task}\nDelivery ID: {delivery}\nWrite optional JSON progress \
         {{\"text\":\"...\"}} to {progress}\nOptionally write the final Markdown result (private mode 0600, up to 64 KiB) to \
         {result}\n\n{prompt}",
        task = req.task_id,
        delivery = req.delivery_id,
        progress = req.progress_path.display(),
        result = req.result_path.display(),
        prompt = req.task.prompt,
    )
}

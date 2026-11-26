//! Plain external-process Adapter: one fixed command per Executor, prompt on stdin.

use std::time::Duration;

use async_trait::async_trait;
use myrlet_protocol::delivery::CancelState;
use myrlet_protocol::id::{ExecutionRef, ExecutorId};
use tokio::sync::mpsc::UnboundedSender;

use super::runner::ProcessRunner;
use super::{AdapterError, ExecutorAdapter, HandlingEnded, SpawnSpec, SubmitRequest};

/// Runs `program` + `args` with the startup prompt on stdin and nothing on stdout:
/// the command reports Progress only through the Progress file.
#[derive(Debug)]
pub struct ProcessAdapter {
    id: ExecutorId,
    program: String,
    args: Vec<String>,
    timeout: Option<Duration>,
    runner: ProcessRunner,
}

impl ProcessAdapter {
    /// `timeout_seconds` bounds one handling attempt; `None` lets the command finish.
    pub fn new(
        id: ExecutorId,
        program: impl Into<String>,
        args: Vec<String>,
        timeout_seconds: Option<u64>,
    ) -> Self {
        Self {
            id,
            program: program.into(),
            args,
            timeout: timeout_seconds.map(Duration::from_secs),
            runner: ProcessRunner::new(),
        }
    }
}

#[async_trait]
impl ExecutorAdapter for ProcessAdapter {
    fn id(&self) -> &ExecutorId {
        &self.id
    }

    fn kind(&self) -> &'static str {
        "process"
    }

    async fn submit(
        &self,
        req: SubmitRequest,
        on_exit: UnboundedSender<HandlingEnded>,
    ) -> Result<ExecutionRef, AdapterError> {
        let prompt = super::build_startup_prompt(&req);
        let spec = SpawnSpec {
            program: self.program.as_str(),
            args: self.args.as_slice(),
            stdin_prompt: Some(prompt.as_str()),
            timeout: self.timeout,
        };
        self.runner
            .spawn(self.id.as_str(), &req.delivery_id, spec, on_exit)
    }

    async fn cancel(&self, execution_ref: &ExecutionRef) -> CancelState {
        self.runner.cancel(execution_ref).await
    }
}

//! Non-invasive Harness CLI Adapter for Claude Code, Codex, and Pi: the only thing the
//! Worker supplies is the Harness' own documented non-interactive invocation. No hooks,
//! plugins, MCP servers, env overrides, Session/Workspace management, or output parsing.

use std::time::Duration;

use async_trait::async_trait;
use myrlet_protocol::delivery::CancelState;
use myrlet_protocol::id::{ExecutionRef, ExecutorId};
use tokio::sync::mpsc::UnboundedSender;

use super::runner::ProcessRunner;
use super::{AdapterError, ExecutorAdapter, HandlingEnded, SpawnSpec, SubmitRequest};

/// The Harness CLI families V1 can start non-interactively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessKind {
    Claude,
    Codex,
    Pi,
}

impl HarnessKind {
    /// Executable name on `PATH`; the operator owns installing and authenticating it.
    pub fn program(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
        }
    }

    /// Inventory kind label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
        }
    }

    /// Arguments preceding the prompt (a subcommand, when the CLI has one).
    fn prefix(&self) -> &'static [&'static str] {
        match self {
            Self::Claude | Self::Pi => &[],
            Self::Codex => &["exec"],
        }
    }

    /// Flag that takes the prompt inline, when the CLI does not accept it positionally.
    fn prompt_flag(&self) -> Option<&'static str> {
        match self {
            Self::Claude | Self::Pi => Some("-p"),
            Self::Codex => None,
        }
    }

    /// Model-selection flag this CLI documents, if it has one.
    fn model_flag(&self) -> Option<&'static str> {
        match self {
            Self::Claude | Self::Codex => Some("--model"),
            Self::Pi => None,
        }
    }

    /// `claude -p <prompt>`, `codex exec <prompt>`, or `pi -p <prompt>`, each with the
    /// requested model only when the CLI accepts one.
    fn command_args(&self, prompt: &str, preferred_model: Option<&str>) -> Vec<String> {
        let mut args: Vec<String> = self.prefix().iter().map(|arg| (*arg).to_owned()).collect();
        if let (Some(flag), Some(model)) = (self.model_flag(), preferred_model) {
            args.push(flag.to_owned());
            args.push(model.to_owned());
        }
        if let Some(flag) = self.prompt_flag() {
            args.push(flag.to_owned());
        }
        args.push(prompt.to_owned());
        args
    }
}

/// Thin Adapter over one Harness CLI; process lifecycle and cancel come from
/// [`ProcessRunner`], so every Harness shares one implementation.
#[derive(Debug)]
pub struct HarnessCliAdapter {
    id: ExecutorId,
    kind: HarnessKind,
    runner: ProcessRunner,
}

impl HarnessCliAdapter {
    pub fn new(id: ExecutorId, kind: HarnessKind) -> Self {
        Self {
            id,
            kind,
            runner: ProcessRunner::new(),
        }
    }
}

#[async_trait]
impl ExecutorAdapter for HarnessCliAdapter {
    fn id(&self) -> &ExecutorId {
        &self.id
    }

    fn kind(&self) -> &'static str {
        self.kind.label()
    }

    async fn submit(
        &self,
        req: SubmitRequest,
        on_exit: UnboundedSender<HandlingEnded>,
    ) -> Result<ExecutionRef, AdapterError> {
        let prompt = super::build_startup_prompt(&req);
        let args = self
            .kind
            .command_args(&prompt, req.task.execution.preferred_model.as_deref());
        let spec = SpawnSpec {
            program: self.kind.program(),
            args: args.as_slice(),
            stdin_prompt: None,
            timeout: req.task.execution.timeout_seconds.map(Duration::from_secs),
        };
        self.runner
            .spawn(self.id.as_str(), &req.delivery_id, spec, on_exit)
    }

    async fn cancel(&self, execution_ref: &ExecutionRef) -> CancelState {
        self.runner.cancel(execution_ref).await
    }
}

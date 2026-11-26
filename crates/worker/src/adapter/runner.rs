//! Shared child-process lifecycle for the Process and Harness CLI Adapters: spawn in a
//! private process group, await exit in a lightweight Tokio task, signal on cancel.
//! No stdout/stderr parsing: Progress only ever comes from the Progress file.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use myrlet_protocol::delivery::CancelState;
use myrlet_protocol::id::{DeliveryId, ExecutionRef};
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use super::{AdapterError, HandlingEnded, SpawnSpec};
use crate::process_group::{configure_process_group, terminate_process_group, CANCEL_GRACE};

/// How long a `cancel` call waits for the exit confirmation it requested.
const CANCEL_RESULT_WAIT: Duration = Duration::from_secs(15);

/// Cancellation rendezvous for one running execution. The exiting task owns the child
/// process; this handle only asks it to stop and reports what it observed.
#[derive(Debug)]
struct CancelHandle {
    request: oneshot::Sender<()>,
    confirmed: oneshot::Receiver<bool>,
}

/// Which event ended one watched execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// The process exited on its own.
    Exited,
    /// A cancel request arrived.
    CancelRequested,
    /// The Task's execution timeout elapsed.
    TimedOut,
}

/// Active spawned children keyed by [`ExecutionRef`].
#[derive(Debug, Default)]
pub struct ProcessRunner {
    active: Arc<Mutex<HashMap<ExecutionRef, CancelHandle>>>,
}

impl ProcessRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn the command in `spec` and report end of handling through `on_exit`.
    pub fn spawn(
        &self,
        executor: &str,
        delivery_id: &DeliveryId,
        spec: SpawnSpec<'_>,
        on_exit: UnboundedSender<HandlingEnded>,
    ) -> Result<ExecutionRef, AdapterError> {
        let mut command = Command::new(spec.program);
        command
            .args(spec.args)
            .stdin(
                spec.stdin_prompt
                    .map_or_else(Stdio::null, |_| Stdio::piped()),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false);
        configure_process_group(&mut command);

        let mut child = command.spawn().map_err(|source| AdapterError::Spawn {
            executor: executor.to_owned(),
            source,
        })?;
        if let Some(text) = spec.stdin_prompt {
            let Some(mut stdin) = child.stdin.take() else {
                let _ = child.start_kill();
                return Err(AdapterError::UncontrollableProcess(executor.to_owned()));
            };
            let bytes = text.as_bytes().to_vec();
            // A short startup prompt fits the pipe buffer; a Harness that exits early
            // closes stdin, which is not a submission failure.
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(&bytes).await;
                let _ = stdin.shutdown().await;
            });
        }

        let execution_ref = ExecutionRef::new(format!("exec_{}", uuid::Uuid::new_v4().simple()));
        let (request, cancel_rx) = oneshot::channel();
        let (confirmed_tx, confirmed) = oneshot::channel();
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(execution_ref.clone(), CancelHandle { request, confirmed });
        let active = Arc::clone(&self.active);
        let watched = execution_ref.clone();
        let watched_delivery = delivery_id.clone();
        let timeout = spec.timeout;
        tokio::spawn(async move {
            // Scoping the pinned wait future releases its borrow of `child` before the
            // outcome is acted on, so the same child can be signalled right after.
            let end = {
                let wait = child.wait();
                tokio::pin!(wait);
                tokio::select! {
                    _ = &mut wait => End::Exited,
                    _ = cancel_rx => End::CancelRequested,
                    _ = sleep_for(timeout) => End::TimedOut,
                }
            };
            match end {
                End::Exited => {}
                End::CancelRequested => {
                    let confirmed_exit = terminate_process_group(&mut child, CANCEL_GRACE).await;
                    let _ = confirmed_tx.send(confirmed_exit);
                }
                End::TimedOut => {
                    let _ = terminate_process_group(&mut child, CANCEL_GRACE).await;
                }
            }
            active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&watched);
            let _ = on_exit.send(HandlingEnded {
                delivery_id: watched_delivery,
                execution_ref: watched,
            });
        });

        Ok(execution_ref)
    }

    /// Cancel one execution: `Confirmed` only when the running child was observed to
    /// exit, else `Uncertain`, because a missing handle proves nothing about side effects.
    pub async fn cancel(&self, execution_ref: &ExecutionRef) -> CancelState {
        let handle = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(execution_ref);
        let Some(handle) = handle else {
            return CancelState::Uncertain;
        };
        if handle.request.send(()).is_err() {
            return CancelState::Uncertain; // handling already ended before the request
        }
        match tokio::time::timeout(CANCEL_RESULT_WAIT, handle.confirmed).await {
            Ok(Ok(true)) => CancelState::Confirmed,
            _ => CancelState::Uncertain,
        }
    }
}

/// Sleep for `timeout`, or never when the Task set no execution timeout.
async fn sleep_for(timeout: Option<Duration>) {
    match timeout {
        Some(duration) => tokio::time::sleep(duration).await,
        None => std::future::pending().await,
    }
}

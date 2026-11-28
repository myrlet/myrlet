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
use crate::process_group::{
    configure_process_group, terminate_process_group, CANCEL_ACK_BOUND, CANCEL_GRACE,
};

/// Cancellation rendezvous for one running execution. The exiting task owns the child
/// process; this handle only asks it to stop and reports what it observed.
#[derive(Debug)]
struct CancelHandle {
    request: oneshot::Sender<()>,
    confirmed: oneshot::Receiver<bool>,
}

/// Which event ended one watched execution.
enum End {
    /// The worker's own `wait` resolved; the result may still be an error.
    Exited(std::io::Result<std::process::ExitStatus>),
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
        // Captured while the child is unreaped: its PID stays allocated, so this is the
        // group ID of exactly this execution and cannot have been reused by another one.
        let pgid = child.id();
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
                    status = &mut wait => End::Exited(status),
                    _ = cancel_rx => End::CancelRequested,
                    _ = sleep_for(timeout) => End::TimedOut,
                }
            };
            // End of handling is only reported when the Worker actually observed the
            // execution end. A failed wait or a process group that resisted termination
            // stays honestly unresolved instead of fabricating a Finish.
            let ended = match end {
                End::Exited(Ok(_)) => true,
                End::Exited(Err(error)) => {
                    tracing::warn!(
                        %error,
                        delivery = %watched_delivery,
                        "execution wait failed; handling stays unresolved"
                    );
                    false
                }
                End::CancelRequested => {
                    let termination = terminate_process_group(&mut child, pgid, CANCEL_GRACE).await;
                    let _ = confirmed_tx.send(termination.confirmed());
                    termination.confirmed()
                }
                End::TimedOut => {
                    let termination = terminate_process_group(&mut child, pgid, CANCEL_GRACE).await;
                    if !termination.confirmed() {
                        tracing::warn!(
                            delivery = %watched_delivery,
                            group_clear = termination.group_clear,
                            "timed-out execution could not be confirmed stopped"
                        );
                    }
                    termination.confirmed()
                }
            };
            active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&watched);
            if ended {
                let _ = on_exit.send(HandlingEnded {
                    delivery_id: watched_delivery,
                    execution_ref: watched,
                });
            }
        });

        Ok(execution_ref)
    }

    /// Cancel one execution: `Confirmed` only when the whole original process group was
    /// observed to disappear, else `Uncertain`, because a missing handle or a merely
    /// delivered signal proves nothing about side effects.
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
        match tokio::time::timeout(CANCEL_ACK_BOUND, handle.confirmed).await {
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

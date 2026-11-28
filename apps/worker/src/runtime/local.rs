//! Owner-only Unix-socket control channel for `myrlet done` and `myrlet result ...`.
//!
//! The CLI never talks to the Hub for execution-local operations. Each request names one
//! Delivery ID; the Worker resolves its own recorded paths, rereads local content, and
//! decides what to reconcile. A request for a Delivery this Worker never registered is
//! rejected, so a late or duplicate command for an old Delivery cannot touch a newer one.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use myrlet_protocol::{result_sha256, DeliveryId, LocalCommand, LocalReply, ResultComparison};
use myrlet_worker::{read_result, LocalDeliveryRecord, LocalResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use super::RuntimeCore;
use crate::reporting::ReportError;

/// Bind the private control socket and serve requests until the process exits.
pub(super) async fn serve(core: Arc<RuntimeCore>, socket_path: &Path) {
    let _ = std::fs::remove_file(socket_path);
    let listener = match UnixListener::bind(socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, path = %socket_path.display(), "control socket could not be bound");
            return;
        }
    };
    if let Err(error) =
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
    {
        tracing::warn!(%error, "control socket permissions could not be restricted");
    }
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let core = Arc::clone(&core);
                tokio::spawn(async move { handle(core, stream).await });
            }
            Err(error) => tracing::debug!(%error, "control connection failed"),
        }
    }
}

/// Serve one CLI request and write one JSON reply line.
async fn handle(core: Arc<RuntimeCore>, stream: UnixStream) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    let reply = match reader.read_line(&mut line).await {
        Ok(0) => return,
        Ok(_) => match myrlet_protocol::parse_request(&line) {
            Ok(command) => answer(&core, command).await,
            Err(error) => LocalReply::rejected_unknown(error.to_string()),
        },
        Err(error) => {
            tracing::debug!(%error, "control request could not be read");
            return;
        }
    };
    if let Ok(bytes) = serde_json::to_vec(&reply) {
        let _ = write_half.write_all(&bytes).await;
        let _ = write_half.write_all(b"\n").await;
        let _ = write_half.flush().await;
    }
}

/// Resolve one command against the local registry and local files.
async fn answer(core: &Arc<RuntimeCore>, command: LocalCommand) -> LocalReply {
    let delivery = DeliveryId::new(command.delivery_id().to_string());
    let record = match core.registry.get(&delivery) {
        Ok(Some(record)) => record,
        Ok(None) => {
            return LocalReply::rejected(
                &command,
                "no locally recorded Delivery has this ID on this Worker",
            )
        }
        Err(error) => return LocalReply::rejected(&command, error.to_string()),
    };
    match command {
        LocalCommand::Done { .. } => {
            if core.epoch() == 0 {
                return LocalReply::rejected(&command, "no live Hub connection; sync will resume");
            }
            // `done` requests reconciliation; it never authorizes Finish, which remains
            // the Worker's own observation of the Harness process ending.
            match core.sync_result_now(&record.delivery_id).await {
                Ok(sha) => LocalReply {
                    hub_confirmed: Some(true),
                    local_sha256: sha.clone(),
                    hub_sha256: sha,
                    outcome: "recorded that handling ended; the Hub holds the current result"
                        .into(),
                    ..LocalReply::accepted(&command, "")
                },
                Err(ReportError::StaleDelivery(_)) => LocalReply::rejected(
                    &command,
                    "the Hub refused this Delivery epoch; reconnect and retry",
                ),
                Err(error) => LocalReply {
                    hub_confirmed: Some(false),
                    outcome: format!("handling recorded; the Hub did not confirm yet: {error}"),
                    ..LocalReply::accepted(&command, "")
                },
            }
        }
        LocalCommand::ResultCheck { .. } => {
            let local = read_result(&record.result_path);
            let warnings = check_markdown(&local);
            LocalReply {
                local_sha256: local_text(&local).map(result_sha256),
                warnings,
                outcome: "advisory only; warnings never block sync or finish".into(),
                ..LocalReply::accepted(&command, "")
            }
        }
        LocalCommand::ResultSync { .. } => {
            if core.epoch() == 0 {
                return LocalReply::rejected(&command, "no live Hub connection; retry later");
            }
            match core.sync_result_now(&record.delivery_id).await {
                Ok(sha) => LocalReply {
                    hub_confirmed: Some(true),
                    local_sha256: sha.clone(),
                    hub_sha256: sha,
                    outcome: "the Hub holds the current local result".into(),
                    ..LocalReply::accepted(&command, "")
                },
                Err(ReportError::StaleDelivery(_)) => LocalReply::rejected(
                    &command,
                    "the Hub refused this Delivery epoch; reconnect and retry",
                ),
                Err(error) => LocalReply {
                    hub_confirmed: Some(false),
                    outcome: format!("the Hub did not confirm: {error}"),
                    ..LocalReply::accepted(&command, "")
                },
            }
        }
        LocalCommand::ResultStatus { .. } => status(core, &command, &record).await,
    }
}

/// Compare the local file with the Hub copy without writing either side.
async fn status(
    core: &Arc<RuntimeCore>,
    command: &LocalCommand,
    record: &LocalDeliveryRecord,
) -> LocalReply {
    let local = read_result(&record.result_path);
    let local_sha = local_text(&local).map(result_sha256);
    let (comparison, hub_sha) = match core.hub_result_sha(&record.delivery_id).await {
        Ok(hub) => (compare(local_sha.as_deref(), hub.as_deref()), hub),
        Err(_) => (ResultComparison::Unknown, None),
    };
    LocalReply {
        hub_confirmed: None,
        outcome: label(comparison).to_string(),
        hub_sha256: hub_sha,
        local_sha256: local_sha,
        ..LocalReply::accepted(command, "")
    }
}

fn compare(local: Option<&str>, hub: Option<&str>) -> ResultComparison {
    match (local, hub) {
        (Some(left), Some(right)) if left == right => ResultComparison::Equal,
        (Some(_), Some(_)) => ResultComparison::Different,
        (Some(_), None) => ResultComparison::MissingRemotely,
        (None, Some(_)) => ResultComparison::MissingLocally,
        (None, None) => ResultComparison::Absent,
    }
}

fn label(comparison: ResultComparison) -> &'static str {
    match comparison {
        ResultComparison::Equal => "equal",
        ResultComparison::Different => "different",
        ResultComparison::MissingRemotely => "missing_remotely",
        ResultComparison::MissingLocally => "missing_locally",
        ResultComparison::Absent => "absent",
        ResultComparison::Unknown => "unknown",
    }
}

fn local_text(local: &LocalResult) -> Option<&str> {
    match local {
        LocalResult::Content(text) => Some(text),
        _ => None,
    }
}

/// Advisory Markdown feedback: never fatal, never blocks sync or Finish.
fn check_markdown(local: &LocalResult) -> Vec<String> {
    match local {
        LocalResult::Unreadable => {
            vec!["the result file could not be read safely; the Hub copy is preserved".into()]
        }
        LocalResult::Absent | LocalResult::Empty => Vec::new(),
        LocalResult::Content(text) => {
            let mut warnings = Vec::new();
            if !text.starts_with('#') {
                warnings.push("the result does not start with a Markdown heading".into());
            }
            if text.lines().count() > 5_000 {
                warnings.push("the result is unusually long (over 5000 lines)".into());
            }
            warnings
        }
    }
}

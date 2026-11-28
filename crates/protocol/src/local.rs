//! Local Worker command channel: the `myrlet` CLI's execution-local operations.
//!
//! A `done <delivery-id>`, `result check/sync/status <delivery-id>` call never touches the
//! Hub directly. It writes one line to the Worker's private control socket; the Worker
//! resolves the recorded Delivery ID, rereads its own files, and performs the request. A
//! Delivery ID from an old execution therefore cannot affect a newer one on the same Slot.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One execution-local request the CLI hands to the running Worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum LocalCommand {
    /// The Harness finished its own work: request Result reconciliation, never termination.
    Done { delivery_id: String },
    /// Advisory Markdown/format feedback; never blocks sync or Finish.
    ResultCheck { delivery_id: String },
    /// Request one reconciliation and report its outcome.
    ResultSync { delivery_id: String },
    /// Compare local and Hub content without writing either side.
    ResultStatus { delivery_id: String },
}

impl LocalCommand {
    /// The Delivery this operation is scoped to.
    pub fn delivery_id(&self) -> &str {
        match self {
            Self::Done { delivery_id }
            | Self::ResultCheck { delivery_id }
            | Self::ResultSync { delivery_id }
            | Self::ResultStatus { delivery_id } => delivery_id,
        }
    }

    /// Stable operation label used in replies and `--json` output.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Done { .. } => "done",
            Self::ResultCheck { .. } => "result.check",
            Self::ResultSync { .. } => "result.sync",
            Self::ResultStatus { .. } => "result.status",
        }
    }
}

/// One local reply. Exit status and `hubConfirmed` are kept separate on purpose so a
/// Harness can tell "the Worker received this" from "the Hub stored this".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalReply {
    pub operation: String,
    pub delivery_id: String,
    /// The Worker accepted and acted on the command.
    pub local_receipt: bool,
    /// The Hub confirmed the resulting value, when a Hub write was involved.
    pub hub_confirmed: Option<bool>,
    /// Human summary; for `result status` this names the comparison.
    pub outcome: String,
    /// Advisory warnings from `result check`; empty for other operations.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// SHA-256 of the Hub content, when the Hub copy is known.
    #[serde(default)]
    pub hub_sha256: Option<String>,
    /// SHA-256 of the local content, when present.
    #[serde(default)]
    pub local_sha256: Option<String>,
}

impl LocalReply {
    pub fn accepted(command: &LocalCommand, outcome: impl Into<String>) -> Self {
        Self {
            operation: command.kind().to_string(),
            delivery_id: command.delivery_id().to_string(),
            local_receipt: true,
            hub_confirmed: None,
            outcome: outcome.into(),
            warnings: Vec::new(),
            hub_sha256: None,
            local_sha256: None,
        }
    }

    pub fn rejected(command: &LocalCommand, outcome: impl Into<String>) -> Self {
        Self {
            local_receipt: false,
            ..Self::accepted(command, outcome)
        }
    }

    /// A rejected reply for a request that could not even be decoded.
    pub fn rejected_unknown(outcome: impl Into<String>) -> Self {
        Self {
            operation: "unknown".to_string(),
            delivery_id: String::new(),
            local_receipt: false,
            hub_confirmed: None,
            outcome: outcome.into(),
            warnings: Vec::new(),
            hub_sha256: None,
            local_sha256: None,
        }
    }
}

/// Resolve the Worker control socket inside `MYRLET_HOME`.
pub fn control_socket_path(worker_dir: &Path) -> PathBuf {
    worker_dir.join("control.sock")
}

/// Send one command to the running Worker and read its reply.
pub fn send(socket: &Path, command: &LocalCommand) -> Result<LocalReply, LocalChannelError> {
    let mut stream = UnixStream::connect(socket).map_err(|source| LocalChannelError::Connect {
        path: socket.to_path_buf(),
        source,
    })?;
    let line = serde_json::to_string(command).map_err(LocalChannelError::Encode)?;
    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|source| LocalChannelError::Io {
            path: socket.to_path_buf(),
            source,
        })?;
    let mut reader = BufReader::new(stream);
    let mut reply = String::new();
    reader
        .read_line(&mut reply)
        .map_err(|source| LocalChannelError::Io {
            path: socket.to_path_buf(),
            source,
        })?;
    serde_json::from_str(reply.trim()).map_err(LocalChannelError::Decode)
}

/// Parse one request line received by the Worker's control socket.
pub fn parse_request(line: &str) -> Result<LocalCommand, LocalChannelError> {
    serde_json::from_str(line.trim()).map_err(LocalChannelError::Decode)
}

/// Local control-channel failures.
#[derive(Debug, thiserror::Error)]
pub enum LocalChannelError {
    #[error("no running Worker control channel at {path:?}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("control channel I/O failed at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("control request could not be encoded")]
    Encode(#[source] serde_json::Error),
    #[error("control reply was not a valid local reply")]
    Decode(#[source] serde_json::Error),
}

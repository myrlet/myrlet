//! Read-only watch over the single Harness-authored Progress file. The Worker creates
//! `worker/progress/` but never creates, writes, or edits `<delivery_id>.json`.

use std::io::ErrorKind;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use myrlet_protocol::progress::ProgressFileDocument;
use sha2::{Digest, Sha256};

/// One newly observed Progress snapshot, ready to be relayed to Hub over HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedProgress {
    /// Strictly increasing per Delivery, so retries can never move Progress backwards.
    pub observed_at_ms: i64,
    pub text: String,
}

/// Read `path` and return its Progress only when the text actually changed.
///
/// A missing file (the Harness has not written Progress yet), a partially written file,
/// or an oversized/invalid document yields `None` instead of an error: absent Progress is
/// normal and must never fabricate execution evidence. `last_hash` tracks the last
/// relayed text so duplicates are suppressed; `last_observed_ms` seeds the monotonic
/// timestamp so a fresh read always sorts after everything already reported.
pub fn read_progress_if_changed(
    path: &Path,
    last_hash: &mut Option<[u8; 32]>,
    last_observed_ms: i64,
) -> Option<ObservedProgress> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "progress file is unreadable");
            return None;
        }
    };
    let document = match ProgressFileDocument::parse_bounded(&bytes) {
        Ok(document) => document,
        Err(error) => {
            // Mid-write partial JSON and invalid documents are expected and ignored.
            tracing::debug!(
                path = %path.display(),
                %error,
                "progress file is not readable yet"
            );
            return None;
        }
    };

    // SHA-256 always yields 32 bytes, so this copy is exact and cannot panic.
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&Sha256::digest(document.text.as_bytes()));
    if last_hash.as_ref() == Some(&digest) {
        return None;
    }
    *last_hash = Some(digest);
    Some(ObservedProgress {
        observed_at_ms: monotonic_now_ms(last_observed_ms),
        text: document.text,
    })
}

/// Wall-clock milliseconds, advanced past `after_ms` when the clock is stale or equal.
fn monotonic_now_ms(after_ms: i64) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0);
    now.max(after_ms.saturating_add(1))
}

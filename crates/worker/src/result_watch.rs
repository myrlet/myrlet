//! Read-only access to the optional Harness-authored Markdown result file. The Worker
//! creates `worker/results/` but never creates, writes, or edits `<delivery_id>.md`.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use myrlet_protocol::result::MAX_RESULT_BYTES;

/// One Worker-side reading of the Result file.
///
/// The distinction matters: only `Absent` or `Empty` may clear the Hub copy. A file that
/// could not be read safely, is oversized, is not UTF-8, or is being written right now is
/// `Unreadable`, because an unreadable file never proves that there is no Result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalResult {
    /// The file does not exist: no Result.
    Absent,
    /// The file exists and holds only whitespace: no Result.
    Empty,
    /// The file holds the current Markdown text.
    Content(String),
    /// The file could not be read safely; the Hub copy must be preserved.
    Unreadable,
}

impl LocalResult {
    /// Text the Hub should hold, or `None` when there is no Result.
    ///
    /// An `Unreadable` reading yields an error so the caller neither writes nor clears.
    pub fn content(&self) -> Result<Option<&str>, LocalResultError> {
        match self {
            Self::Absent | Self::Empty => Ok(None),
            Self::Content(text) => Ok(Some(text)),
            Self::Unreadable => Err(LocalResultError::Incomplete),
        }
    }
}

#[cfg(unix)]
fn open_result(path: &Path) -> io::Result<File> {
    use rustix::fs::{open, openat, Mode, OFlags};
    use std::os::unix::fs::PermissionsExt;

    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let dir = File::from(open(
        path.parent().ok_or(io::ErrorKind::InvalidInput)?,
        flags | OFlags::DIRECTORY,
        Mode::empty(),
    )?);
    if dir.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let file = File::from(openat(
        &dir,
        path.file_name().ok_or(io::ErrorKind::InvalidInput)?,
        flags | OFlags::NONBLOCK,
        Mode::empty(),
    )?);
    if file.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_result(path: &Path) -> io::Result<File> {
    myrlet_protocol::paths::verify_private_file(path).map_err(io::Error::other)?;
    File::open(path)
}

/// Read the current Result file, classifying absence separately from an unsafe read.
///
/// A missing file is the normal "no Result" case. Anything else that prevents a complete
/// read is `Unreadable`, so synchronization preserves whatever the Hub already holds.
pub fn read_result(path: &Path) -> LocalResult {
    let read = || -> io::Result<LocalResult> {
        let file = open_result(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_RESULT_BYTES as u64 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut text = String::new();
        file.take(MAX_RESULT_BYTES as u64 + 1)
            .read_to_string(&mut text)?;
        if text.len() > MAX_RESULT_BYTES {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(if text.trim().is_empty() {
            LocalResult::Empty
        } else {
            LocalResult::Content(text)
        })
    };
    match read() {
        Ok(reading) => reading,
        Err(error) if error.kind() == io::ErrorKind::NotFound => LocalResult::Absent,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "result file could not be read safely");
            LocalResult::Unreadable
        }
    }
}

/// Failure to establish the local Result value.
#[derive(Debug, thiserror::Error)]
pub enum LocalResultError {
    #[error("the local result file could not be read completely")]
    Incomplete,
}

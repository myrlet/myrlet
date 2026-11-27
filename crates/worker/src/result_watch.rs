//! Read-only access to the optional Harness-authored Markdown result file. The Worker
//! creates `worker/results/` but never creates, writes, or edits `<delivery_id>.md`.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use myrlet_protocol::result::MAX_RESULT_BYTES;

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

/// Capture a bounded, private regular file once, when local handling ends.
pub fn read_result(path: &Path) -> Option<String> {
    let read = || -> io::Result<Option<String>> {
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
        Ok((!text.trim().is_empty()).then_some(text))
    };
    match read() {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "ignored invalid result file");
            None
        }
    }
}

//! `MYRLET_HOME` canonical layout (`local-runtime.md`) and the fail-closed path,
//! symlink, and permission rules of `security.md`.

use crate::id::DeliveryId;
use std::fs::{self, Metadata};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

const ENV_MYRLET_HOME: &str = "MYRLET_HOME";

/// Declares `root`-relative accessors so the layout has exactly one definition.
macro_rules! layout {
    ($($(#[$meta:meta])* $name:ident => $($segment:literal).+;)*) => {$(
        $(#[$meta])*
        pub fn $name(&self) -> PathBuf {
            self.root$(.join($segment))+
        }
    )*};
}

/// Failure to resolve or safely use a Myrlet-managed path.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("MYRLET_HOME is empty or unset")]
    Empty,
    #[error("path must be absolute")]
    RelativePath,
    #[error("path must not contain parent-directory (`..`) components")]
    ParentTraversal,
    #[error("path must not be inside a Git repository")]
    InsideGitRepo,
    #[error("managed path must not be a symlink")]
    Symlink,
    #[error("managed path must be a directory")]
    NotDirectory,
    #[error("managed path must be a regular file")]
    NotRegularFile,
    #[error("managed path {path:?} is accessible by group or others (mode {mode:o})")]
    PermissiveMode { path: PathBuf, mode: u32 },
    #[error("I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Resolved `MYRLET_HOME` root with the canonical child paths.
#[derive(Debug, Clone)]
pub struct MyrletHome {
    root: PathBuf,
}

impl MyrletHome {
    /// Resolve from an explicit override, else `MYRLET_HOME`. The root must be
    /// absolute, free of `..`, outside a Git checkout, unsymlinked, and a directory.
    pub fn resolve(override_root: Option<&Path>) -> Result<Self, PathError> {
        let root = match override_root {
            Some(path) => path.to_path_buf(),
            None => match std::env::var_os(ENV_MYRLET_HOME) {
                Some(value) if !value.is_empty() => PathBuf::from(value),
                _ => return Err(PathError::Empty),
            },
        };
        if root.as_os_str().is_empty() {
            return Err(PathError::Empty);
        }
        if !root.is_absolute() {
            return Err(PathError::RelativePath);
        }
        if root.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(PathError::ParentTraversal);
        }

        // Symlink/non-directory checks come first: a bad root must never be
        // masked by (or reported as) the Git-repository check. Mode is enforced
        // on the managed children (`ensure_private_dir`), not on the operator root.
        match fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(PathError::Symlink),
            Ok(metadata) if !metadata.is_dir() => return Err(PathError::NotDirectory),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(source) => return Err(io_error(&root, source)),
        }
        if is_inside_git_repo(&root) {
            return Err(PathError::InsideGitRepo);
        }
        Ok(Self { root })
    }

    /// Data root itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    layout! {
        /// `hub/`.
        hub_dir => "hub";
        /// `hub/config.toml`.
        hub_config => "hub"."config.toml";
        /// `hub/credentials.json` (mode `0600`).
        hub_credentials => "hub"."credentials.json";
        /// `hub/hub.sqlite` (mode `0600`).
        hub_database => "hub"."hub.sqlite";
        /// `worker/`.
        worker_dir => "worker";
        /// `worker/worker.yaml`.
        worker_config => "worker"."worker.yaml";
        /// `worker/credentials.json` (mode `0600`).
        worker_credentials => "worker"."credentials.json";
        /// `worker/state/`.
        worker_state_dir => "worker"."state";
        /// `worker/progress/`.
        worker_progress_dir => "worker"."progress";
    }

    /// `worker/progress/<delivery_id>.json`; the Delivery ID must be one safe path segment.
    pub fn worker_progress_file(&self, delivery_id: &DeliveryId) -> Result<PathBuf, PathError> {
        if !delivery_id.is_valid_segment() {
            return Err(PathError::ParentTraversal);
        }
        Ok(self
            .worker_progress_dir()
            .join(format!("{delivery_id}.json")))
    }
}

/// Create a `0700` dir if absent, else verify it; symlinked, non-dir, or group/world-
/// accessible dirs fail closed and are never `chmod`-ed. Always re-verifies.
pub fn ensure_private_dir(path: &Path) -> Result<(), PathError> {
    if is_missing(path) {
        create_private_dir(path)?;
    }
    verify_entry(path, true)
}

/// Verify an existing file is private (`0600`-equivalent); missing files pass, while
/// symlinks, non-files, and permissive modes fail closed unmodified.
pub fn verify_private_file(path: &Path) -> Result<(), PathError> {
    if is_missing(path) {
        return Ok(());
    }
    verify_entry(path, false)
}

/// Atomically write `bytes` to a `0600` file in a private parent: `verify_private_file`
/// vets the destination (symlink/mode) first, then a synced `0600` sibling is renamed in.
pub fn write_private_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), PathError> {
    let parent = path.parent().ok_or(PathError::NotDirectory)?;
    ensure_private_dir(parent)?;
    verify_private_file(path)?; // existing destinations must already be symlink-free & private

    let (temp_path, mut file) = create_temp_sibling(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(source) = written {
        let _ = fs::remove_file(&temp_path);
        return Err(io_error(&temp_path, source));
    }
    if let Err(source) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(io_error(path, source));
    }
    Ok(())
}

/// Verify the SQLite database file and any existing `-wal` / `-shm` sidecars.
pub fn verify_sqlite_sidecars(db_path: &Path) -> Result<(), PathError> {
    for suffix in ["", "-wal", "-shm"] {
        let mut sidecar = db_path.as_os_str().to_owned();
        sidecar.push(suffix);
        verify_private_file(Path::new(&sidecar))?;
    }
    Ok(())
}

fn io_error(path: &Path, source: std::io::Error) -> PathError {
    PathError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `true` only for an absent path; broken links still count as present.
fn is_missing(path: &Path) -> bool {
    matches!(fs::symlink_metadata(path), Err(error) if error.kind() == ErrorKind::NotFound)
}

/// Re-read `path` and require a non-symlink, owner-only directory (or regular file).
fn verify_entry(path: &Path, want_dir: bool) -> Result<(), PathError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| io_error(path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(PathError::Symlink);
    }
    if metadata.is_dir() != want_dir || (!want_dir && !metadata.is_file()) {
        return Err(if want_dir {
            PathError::NotDirectory
        } else {
            PathError::NotRegularFile
        });
    }
    verify_owner_only(path, &metadata)
}

fn verify_owner_only(path: &Path, metadata: &Metadata) -> Result<(), PathError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(PathError::PermissiveMode {
                path: path.to_path_buf(),
                mode,
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, metadata); // no POSIX mode bits; ACLs are platform-specific
    }
    Ok(())
}

/// Create a `0700` dir (never recursive: parents are verified separately).
fn create_private_dir(path: &Path) -> Result<(), PathError> {
    #[cfg(unix)]
    let created = {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    };
    #[cfg(not(unix))]
    let created = fs::DirBuilder::new().create(path);
    created.map_err(|source| io_error(path, source))
}

/// Create a unique hidden sibling temp file with `0600`: `create_new` never adopts a
/// pre-existing file's wider mode, and the sibling keeps the `rename` atomic.
fn create_temp_sibling(path: &Path) -> Result<(PathBuf, fs::File), PathError> {
    let parent = path.parent().ok_or(PathError::NotDirectory)?;
    let name = path.file_name().ok_or(PathError::NotRegularFile)?;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    for attempt in 0..5 {
        let temp = parent.join(format!(
            ".{}.tmp.{attempt}.{:016x}",
            name.to_string_lossy(),
            rand::random::<u64>()
        ));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(io_error(&temp, source)),
        }
    }
    Err(io_error(
        path,
        std::io::Error::new(ErrorKind::AlreadyExists, "temp name collision"),
    ))
}

/// Walk ancestors for `.git` via `symlink_metadata`: dangling links still mark a checkout.
fn is_inside_git_repo(path: &Path) -> bool {
    path.ancestors()
        .any(|dir| fs::symlink_metadata(dir.join(".git")).is_ok())
}

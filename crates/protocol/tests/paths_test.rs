//! Fail-closed coverage for `MYRLET_HOME` resolution and managed-path checks.

use myrlet_protocol::paths::{
    ensure_private_dir, verify_private_file, verify_sqlite_sidecars, write_private_file_atomic,
};
use myrlet_protocol::{DeliveryId, MyrletHome, PathError};
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
}

#[test]
fn rejects_relative_traversal_and_non_directory_roots() {
    let dir = tempdir();
    let file = dir.path().join("file");
    fs::write(&file, b"x").expect("write file");
    assert!(matches!(
        MyrletHome::resolve(Some(Path::new(""))),
        Err(PathError::Empty)
    ));
    assert!(matches!(
        MyrletHome::resolve(Some(Path::new("relative/home"))),
        Err(PathError::RelativePath)
    ));
    let escape = dir.path().join("..").join("myrlet");
    assert!(matches!(
        MyrletHome::resolve(Some(&escape)),
        Err(PathError::ParentTraversal)
    ));
    assert!(matches!(
        MyrletHome::resolve(Some(&file)),
        Err(PathError::NotDirectory)
    ));
    // An absent absolute root is legal: managed children are created on first use.
    assert!(MyrletHome::resolve(Some(&dir.path().join("fresh"))).is_ok());
}

#[test]
fn rejects_roots_inside_a_git_checkout() {
    let dir = tempdir();
    fs::create_dir(dir.path().join(".git")).expect("create .git");
    assert!(matches!(
        MyrletHome::resolve(Some(dir.path())),
        Err(PathError::InsideGitRepo)
    ));
    let nested = dir.path().join("state").join("myrlet");
    fs::create_dir_all(&nested).expect("create nested");
    assert!(matches!(
        MyrletHome::resolve(Some(&nested)),
        Err(PathError::InsideGitRepo)
    ));
}

#[test]
#[cfg(unix)]
fn rejects_symlinked_root_and_file() {
    let dir = tempdir();
    let real = dir.path().join("real");
    fs::create_dir(&real).expect("real dir");
    let root_link = dir.path().join("root-link");
    symlink(&real, &root_link).expect("symlink root");
    let root_err = MyrletHome::resolve(Some(&root_link));
    assert!(matches!(root_err, Err(PathError::Symlink)));
    let dangling = dir.path().join("dangling");
    symlink(dir.path().join("nowhere"), &dangling).expect("dangling symlink");
    assert!(matches!(
        MyrletHome::resolve(Some(&dangling)),
        Err(PathError::Symlink)
    ));
    let private = dir.path().join("private");
    ensure_private_dir(&private).expect("private dir");
    let file = private.join("credentials.json");
    symlink(private.join("target"), &file).expect("symlink file");
    assert!(matches!(
        verify_private_file(&file),
        Err(PathError::Symlink)
    ));
    assert!(matches!(
        write_private_file_atomic(&file, b"{}"),
        Err(PathError::Symlink)
    ));
}

#[test]
#[cfg(unix)]
fn permissive_dir_and_file_fail_closed_without_chmod() {
    let dir = tempdir();
    let permissive_dir = dir.path().join("hub");
    fs::create_dir(&permissive_dir).expect("create dir");
    set_mode(&permissive_dir, 0o755);
    let dir_err = ensure_private_dir(&permissive_dir);
    assert!(matches!(
        dir_err,
        Err(PathError::PermissiveMode { mode: 0o755, .. })
    ));
    assert_eq!(
        mode_of(&permissive_dir),
        0o755,
        "must not rewrite operator dirs"
    );
    let private = dir.path().join("private");
    ensure_private_dir(&private).expect("private dir");
    let credentials = private.join("credentials.json");
    fs::write(&credentials, b"{}").expect("write file");
    set_mode(&credentials, 0o644);
    let verify_err = verify_private_file(&credentials);
    assert!(matches!(
        verify_err,
        Err(PathError::PermissiveMode { mode: 0o644, .. })
    ));
    let write_err = write_private_file_atomic(&credentials, b"{}");
    assert!(matches!(
        write_err,
        Err(PathError::PermissiveMode { mode: 0o644, .. })
    ));
    assert_eq!(
        mode_of(&credentials),
        0o644,
        "must not rewrite operator files"
    );
}

#[test]
#[cfg(unix)]
fn creates_private_dir_and_writes_atomic_0600_files() {
    let dir = tempdir();
    let managed = dir.path().join("worker").join("state");
    fs::create_dir_all(managed.parent().expect("parent")).expect("create parent");
    ensure_private_dir(&managed).expect("create private dir");
    assert_eq!(mode_of(&managed) & 0o077, 0, "dir must be owner-only");
    ensure_private_dir(&managed).expect("existing private dir verifies");
    let progress = dir.path().join("progress");
    let target = progress.join("delivery.json");
    write_private_file_atomic(&target, b"{\"pct\":1}").expect("atomic write");
    assert_eq!(fs::read(&target).expect("read back"), b"{\"pct\":1}");
    assert_eq!(mode_of(&target), 0o600);
    write_private_file_atomic(&target, b"{\"pct\":2}").expect("atomic overwrite");
    assert_eq!(fs::read(&target).expect("read back"), b"{\"pct\":2}");
    assert_eq!(mode_of(&target), 0o600);
    let entries: Vec<_> = fs::read_dir(&progress)
        .expect("read_dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        entries,
        vec![std::ffi::OsString::from("delivery.json")],
        "no temp leftovers"
    );

    // SQLite sidecars: absent is fine, permissive `-wal` fails closed.
    let db = dir.path().join("hub.sqlite");
    fs::write(&db, b"").expect("write db");
    set_mode(&db, 0o600);
    verify_sqlite_sidecars(&db).expect("db without sidecars is fine");
    let wal = dir.path().join("hub.sqlite-wal");
    fs::write(&wal, b"").expect("write wal");
    set_mode(&wal, 0o644);
    assert!(matches!(
        verify_sqlite_sidecars(&db),
        Err(PathError::PermissiveMode { .. })
    ));
}

#[test]
fn layout_and_progress_paths_match_contract() {
    let dir = tempdir();
    let home = MyrletHome::resolve(Some(dir.path())).expect("resolve root");
    assert_eq!(home.root(), dir.path());
    let paths = [
        (home.hub_dir(), "hub"),
        (home.hub_config(), "hub/config.toml"),
        (home.hub_credentials(), "hub/credentials.json"),
        (home.hub_database(), "hub/hub.sqlite"),
        (home.worker_dir(), "worker"),
        (home.worker_config(), "worker/worker.yaml"),
        (home.worker_credentials(), "worker/credentials.json"),
        (home.worker_state_dir(), "worker/state"),
        (home.worker_progress_dir(), "worker/progress"),
        (home.worker_results_dir(), "worker/results"),
    ];
    for (actual, relative) in paths {
        assert_eq!(actual, dir.path().join(relative));
    }
    let delivery = DeliveryId::new("del_01-abc");
    assert_eq!(
        home.worker_progress_file(&delivery).expect("safe id"),
        dir.path().join("worker/progress/del_01-abc.json")
    );
    assert_eq!(
        home.worker_result_file(&delivery).expect("safe id"),
        dir.path().join("worker/results/del_01-abc.md")
    );
    for unsafe_id in ["..", ".", "a/b", ""] {
        let unsafe_path = home.worker_progress_file(&DeliveryId::new(unsafe_id));
        assert!(matches!(unsafe_path, Err(PathError::ParentTraversal)));
        let unsafe_result = home.worker_result_file(&DeliveryId::new(unsafe_id));
        assert!(matches!(unsafe_result, Err(PathError::ParentTraversal)));
    }
}

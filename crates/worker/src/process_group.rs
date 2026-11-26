//! Unix process-group ownership: every spawned Harness runs in its own group so one
//! cancel signals the whole tree, plus a portable child-level fallback elsewhere.

use std::time::Duration;

use tokio::process::{Child, Command};

/// Grace period between `SIGTERM` and `SIGKILL` when terminating an execution.
pub const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// Start each child in a fresh process group so descendants share one cancel target.
pub fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        // SAFETY: `pre_exec` runs this closure in the forked child between `fork` and
        // `exec`. The closure only calls the async-signal-safe `setpgid` syscall, allocates
        // nothing, and touches no shared state, so it cannot deadlock or corrupt the child.
        unsafe {
            command.pre_exec(setpgid_self);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

/// `setpgid(0, 0)` in the forked child: become the leader of a new process group.
#[cfg(unix)]
fn setpgid_self() -> std::io::Result<()> {
    // SAFETY: `setpgid` takes two integers and has no memory-safety preconditions.
    if unsafe { libc::setpgid(0, 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Terminate one execution's process group and report whether the child exited.
///
/// Unix: `SIGTERM` the whole group led by the child, wait at most `grace`, then
/// `SIGKILL`. Other platforms: best-effort `kill()` of the child. Taking the child
/// (rather than a bare `pgid`) is what lets this confirm a real exit instead of
/// reporting a signal was merely delivered.
pub async fn terminate_process_group(child: &mut Child, grace: Duration) -> bool {
    #[cfg(unix)]
    {
        let Some(pgid) = child.id().map(|id| id as i32) else {
            return true; // already reaped: nothing left to terminate
        };
        signal_group(pgid, libc::SIGTERM);
        if tokio::time::timeout(grace, child.wait()).await.is_ok() {
            return true;
        }
        signal_group(pgid, libc::SIGKILL);
        tokio::time::timeout(grace, child.wait()).await.is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = grace;
        child.kill().await.is_ok()
    }
}

/// Signal the whole process group led by `pgid`; nothing to do once the group is gone.
#[cfg(unix)]
fn signal_group(pgid: i32, signal: libc::c_int) {
    if pgid <= 0 {
        return;
    }
    // SAFETY: a negative pid targets the process group this child leads (`setpgid(0, 0)`);
    // `kill` takes two integers and has no memory-safety preconditions.
    unsafe {
        libc::kill(-pgid, signal);
    }
}

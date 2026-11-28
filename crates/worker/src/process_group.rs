//! Unix process-group ownership: every spawned Harness runs in its own group so one
//! cancel signals the whole tree, plus a portable child-level fallback elsewhere.
//!
//! Signalling is scoped to the original execution. While the leader is unreaped its PID
//! (and therefore its process-group ID) stays allocated, so `kill(-pgid, ..)` cannot reach
//! an unrelated group. Every real signal below is sent before the leader is reaped; after
//! reaping, this module only probes emptiness (`signal 0`).

use std::time::Duration;

use tokio::process::{Child, Command};

/// Wait for the leader to exit after `SIGTERM`.
pub const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// Wait for the leader to exit after the escalation to `SIGKILL`. A killed process exits
/// immediately, so this only has to absorb scheduling latency.
pub const KILL_GRACE: Duration = Duration::from_secs(2);
/// Probe-only settle after the escalation to `SIGKILL`, so descendants can be reaped by
/// their new parent before emptiness is judged.
pub const GROUP_SETTLE: Duration = Duration::from_secs(2);
/// Reap the already-exited leader; this needs latency headroom, not a full grace period.
pub const REAP_GRACE: Duration = Duration::from_secs(1);

/// Worst case for one full termination: the TERM wait, the KILL wait, the reap, and the
/// group settle.
pub const TERMINATION_BOUND: Duration = CANCEL_GRACE
    .saturating_add(KILL_GRACE)
    .saturating_add(REAP_GRACE)
    .saturating_add(GROUP_SETTLE);

/// How long `cancel` waits for the exit confirmation it requested. It must exceed
/// [`TERMINATION_BOUND`], otherwise cleanup still inside its own budget would be reported
/// as `Uncertain` even though it went on to succeed.
pub const CANCEL_ACK_BOUND: Duration = Duration::from_secs(12);

const _: () = assert!(CANCEL_ACK_BOUND.as_secs() > TERMINATION_BOUND.as_secs());

/// How often the leader and the group are re-checked while terminating.
#[cfg(unix)]
const QUIESCE_POLL: Duration = Duration::from_millis(25);

/// What one termination attempt actually observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termination {
    /// The spawned leader was observed to exit and was reaped.
    pub leader_exited: bool,
    /// No process remains in the original process group.
    pub group_clear: bool,
}

impl Termination {
    /// A cancel may only be reported as confirmed when the whole group is gone.
    pub fn confirmed(self) -> bool {
        self.leader_exited && self.group_clear
    }
}

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

/// Terminate one execution's process group and report exactly what was observed.
///
/// Unix: `SIGTERM` the whole group, wait for the leader to exit, then `SIGKILL` the whole
/// group so a same-group descendant that ignored `SIGTERM` cannot survive. Confirmation
/// requires the leader to be reaped **and** the group to be empty.
///
/// Other platforms: best-effort `kill()` of the child and wait for it.
pub async fn terminate_process_group(
    child: &mut Child,
    pgid: Option<u32>,
    grace: Duration,
) -> Termination {
    #[cfg(unix)]
    {
        // Captured while this child was unreaped in `spawn`, so this is the group ID of
        // exactly this execution. Without a known PGID we must not signal a group.
        let Some(pid) = pgid
            .and_then(|id| i32::try_from(id).ok())
            .filter(|id| *id > 0)
        else {
            let leader_exited = reap_leader(child, grace).await;
            return Termination {
                leader_exited,
                group_clear: false,
            };
        };
        signal_group(pid, libc::SIGTERM);
        let mut exited = await_leader_exit(pid, grace).await;
        if !exited {
            // Still unreaped, so the PGID is pinned to this execution.
            signal_group(pid, libc::SIGKILL);
            exited = await_leader_exit(pid, KILL_GRACE).await;
        }
        if !exited {
            // The leader survived both signals (for example an uninterruptible task).
            // It is never reaped here, so it stays honestly unresolved; no wait is spawned.
            return Termination {
                leader_exited: false,
                group_clear: false,
            };
        }
        // The leader is a zombie and still pins the PGID; this kills descendants that
        // ignored `SIGTERM` without any risk of reaching a different process group.
        signal_group(pid, libc::SIGKILL);
        let leader_exited = reap_leader(child, REAP_GRACE).await;
        let group_clear = leader_exited && await_group_gone(pid, GROUP_SETTLE).await;
        Termination {
            leader_exited,
            group_clear,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pgid;
        let _ = child.start_kill();
        let leader_exited = tokio::time::timeout(grace, child.wait()).await.is_ok();
        Termination {
            leader_exited,
            group_clear: leader_exited,
        }
    }
}

/// Whether the direct child `pid` has exited, without reaping it.
#[cfg(unix)]
fn leader_exited(pid: i32) -> bool {
    // SAFETY: `waitid` only writes into the provided `siginfo_t`, which is zero-initialized
    // for this call and lives for its duration.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let result = libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        );
        // `P_PID` targets exactly this child; a non-zero `si_signo` reports its state
        // change. Zero means "still running or not yet reported".
        result == 0 && info.si_signo != 0
    }
}

/// Poll `leader_exited` until it reports an exit or `budget` elapses.
#[cfg(unix)]
async fn await_leader_exit(pid: i32, budget: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        if leader_exited(pid) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(QUIESCE_POLL).await;
    }
}

/// Wait for the group to disappear, bounded by `budget`. Never signals the group, so a
/// reaped PGID being reused cannot hurt an unrelated process.
#[cfg(unix)]
async fn await_group_gone(pgid: i32, budget: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        if !group_exists(pgid) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(QUIESCE_POLL).await;
    }
}

/// Reap the leader within `budget`; an `Err` from `wait` is not proof of exit.
#[cfg(unix)]
async fn reap_leader(child: &mut Child, budget: Duration) -> bool {
    match tokio::time::timeout(budget, child.wait()).await {
        Ok(Ok(_)) => true,
        Ok(Err(error)) => {
            tracing::warn!(%error, "the execution leader could not be waited for");
            false
        }
        Err(_) => false,
    }
}

/// Whether any process still belongs to the process group led by `pgid`.
#[cfg(unix)]
fn group_exists(pgid: i32) -> bool {
    if pgid <= 0 {
        return false;
    }
    // SAFETY: `kill` takes two integers and has no memory-safety preconditions; signal 0
    // only probes for existence.
    let result = unsafe { libc::kill(-pgid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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

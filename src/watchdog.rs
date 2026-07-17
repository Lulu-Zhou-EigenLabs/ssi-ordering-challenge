//! Time-bounded child supervision for the local harness. Spawns a child
//! process, polls it, and SIGKILLs it if it exceeds the per-matrix time cap —
//! the same enforcement mechanism the grader uses (grader/runner/src/watchdog.rs),
//! ported `std`-only into the public harness. Command-agnostic so it is
//! unit-testable with /bin/sleep. Trusted harness code.

use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

pub struct CapConfig {
    pub time_cap: Duration,
    pub poll: Duration,
}

impl Default for CapConfig {
    fn default() -> Self {
        CapConfig {
            time_cap: Duration::from_secs(2),
            poll: Duration::from_millis(10),
        }
    }
}

/// Outcome of one supervised child run.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerOutcome {
    /// Child exited 0.
    Ok,
    /// Child exceeded the time cap and was killed.
    Timeout,
    /// Child exited non-zero or could not be spawned/waited (reason attached).
    Crashed(String),
}

/// Spawn `cmd` and supervise it under `cfg`. Kills the child on time breach.
///
/// On unix the worker is placed in its own process group (`process_group(0)`)
/// so a time-cap breach kills the WHOLE group, not just the direct worker pid.
/// Untrusted `order()` can spawn children; killing only the worker would
/// re-parent them to init and let them outlive the cap (issue #17).
pub fn run_capped(cmd: &mut Command, cfg: &CapConfig) -> WorkerOutcome {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // pgid = 0 → new group whose id equals the worker pid.
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return WorkerOutcome::Crashed(format!("could not spawn worker: {e}")),
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    WorkerOutcome::Ok
                } else {
                    WorkerOutcome::Crashed(describe_status(&status))
                };
            }
            Ok(None) => {}
            Err(e) => return WorkerOutcome::Crashed(format!("wait failed: {e}")),
        }
        if start.elapsed() > cfg.time_cap {
            kill_group(&mut child);
            let _ = child.wait();
            return WorkerOutcome::Timeout;
        }
        sleep(cfg.poll);
    }
}

/// SIGKILL the worker and every process it spawned. On unix the worker leads
/// its own process group (see `run_capped`), so `kill -KILL -<pgid>` reaps the
/// whole subtree; `pgid` equals the worker pid we set with `process_group(0)`.
/// Falls back to a single-pid kill on non-unix.
#[cfg(unix)]
fn kill_group(child: &mut std::process::Child) {
    let pid = child.id();
    // Negative target = process group. Std-only: shell out to `kill`, matching
    // the grader watchdog family's use of `ps`/`kill`.
    let killed = Command::new("kill")
        .arg("-KILL")
        .arg(format!("-{pid}"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !killed {
        // Group kill failed (e.g. worker died before setpgid took effect);
        // fall back to signalling the worker pid directly.
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn kill_group(child: &mut std::process::Child) {
    let _ = child.kill();
}

#[cfg(unix)]
fn describe_status(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    if let Some(sig) = status.signal() {
        format!("worker killed by signal {sig}")
    } else if let Some(code) = status.code() {
        format!("worker exited with code {code}")
    } else {
        "worker exited abnormally".to_string()
    }
}

#[cfg(not(unix))]
fn describe_status(status: &std::process::ExitStatus) -> String {
    format!("worker exited: {status}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_command_is_ok() {
        let mut cmd = Command::new("true");
        let cfg = CapConfig {
            time_cap: Duration::from_secs(2),
            poll: Duration::from_millis(5),
        };
        assert_eq!(run_capped(&mut cmd, &cfg), WorkerOutcome::Ok);
    }

    #[test]
    fn slow_command_times_out_promptly() {
        // sleep 10s under a 0.2s cap: must return Timeout well before 10s.
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let cfg = CapConfig {
            time_cap: Duration::from_millis(200),
            poll: Duration::from_millis(5),
        };
        let start = Instant::now();
        let outcome = run_capped(&mut cmd, &cfg);
        assert_eq!(outcome, WorkerOutcome::Timeout);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "watchdog did not kill promptly: {:?}",
            start.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_spawned_grandchildren() {
        // The worker (sh) backgrounds a long sleep, records its pid, then blocks
        // past the cap. Before the fix the watchdog SIGKILLs only sh; the
        // backgrounded sleep is re-parented to init and outlives the cap
        // (issue #17). The fix runs the worker in its own process group and
        // kills the whole group.
        let pidfile = std::env::temp_dir().join(format!("ssi-wd-{}.pid", std::process::id()));
        let _ = std::fs::remove_file(&pidfile);
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(format!("sleep 30 & echo $! > {}; sleep 30", pidfile.display()));
        let cfg = CapConfig {
            time_cap: Duration::from_millis(200),
            poll: Duration::from_millis(5),
        };
        assert_eq!(run_capped(&mut cmd, &cfg), WorkerOutcome::Timeout);

        // Read the grandchild pid the worker recorded before it was killed.
        let mut pid_str = String::new();
        for _ in 0..50 {
            if let Ok(s) = std::fs::read_to_string(&pidfile) {
                if !s.trim().is_empty() {
                    pid_str = s;
                    break;
                }
            }
            sleep(Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(&pidfile);
        let gpid: i32 = pid_str.trim().parse().expect("grandchild pid recorded");

        // The grandchild must be gone shortly after the cap kill. `kill -0`
        // probes existence without signaling.
        let mut alive = true;
        for _ in 0..100 {
            let ok = Command::new("kill")
                .arg("-0")
                .arg(gpid.to_string())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                alive = false;
                break;
            }
            sleep(Duration::from_millis(10));
        }
        if alive {
            // Reap the orphan so a red test doesn't leave it lingering.
            let _ = Command::new("kill").arg("-KILL").arg(gpid.to_string()).status();
        }
        assert!(!alive, "grandchild {gpid} survived the time cap (orphaned)");
    }

    #[test]
    fn nonzero_exit_is_crashed() {
        let mut cmd = Command::new("false");
        let cfg = CapConfig {
            time_cap: Duration::from_secs(2),
            poll: Duration::from_millis(5),
        };
        assert!(matches!(
            run_capped(&mut cmd, &cfg),
            WorkerOutcome::Crashed(_)
        ));
    }
}

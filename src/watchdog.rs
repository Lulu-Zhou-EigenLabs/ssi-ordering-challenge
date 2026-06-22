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
pub fn run_capped(cmd: &mut Command, cfg: &CapConfig) -> WorkerOutcome {
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
            let _ = child.kill();
            let _ = child.wait();
            return WorkerOutcome::Timeout;
        }
        sleep(cfg.poll);
    }
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

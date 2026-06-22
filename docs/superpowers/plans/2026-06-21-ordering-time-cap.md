# Prompt Subprocess-Enforced Ordering Time Cap — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the local harness run each contestant `order()` in a child process and SIGKILL it the moment it exceeds a 2 s per-matrix cap, reporting an explicit, actionable error — so a runaway ordering fails in ~2 s instead of blocking the whole run.

**Architecture:** The harness binary gains two modes. **Worker mode** (`--worker <jsonl_path> <line_index> <out_perm>`) is the only process that runs contestant code: it loads one pattern via `ssi_scoring::load_pattern_jsonl_line`, runs `ordering::order()`, and writes the permutation to a file. **Parent mode** (default) computes the AMD baseline + score in-process, then spawns itself as a worker (twice, for the determinism gate) under a watchdog that SIGKILLs the child at the cap. Two new `std`-only trusted modules — `src/watchdog.rs` (spawn/poll/kill) and `src/perm_io.rs` (binary permutation I/O) — are ported from the grader so the local harness mirrors the grader's enforcement mechanism.

**Tech Stack:** Rust (stable, edition 2021), `std` only (no new dependencies), `cargo test`. Spec: `docs/superpowers/specs/2026-06-21-ordering-time-cap-design.md`.

## Global Constraints

- **THE CONTRACT IS FROZEN (Invariant 1):** do not change the `order(pattern: &Pattern) -> Vec<usize>` signature, the score definition, the validity gates' *semantics* (a cap breach still FAILs the whole run), or the `score.json` / `results.tsv` output formats. The cap *value* is an operating parameter and may move within the cost doc's 2–5 s band.
- **CAP VALUE:** flat **2 seconds** per `order()` call. This is *stricter* than the grader's current 5 s default (the safe direction: passes-locally ⇒ passes-server). Document it; do not change the grader.
- **SUBMISSION DIRECTORY UNTOUCHED (Invariant 3):** do not modify `src/ordering/` except the one env-gated test hook explicitly specified in Task 5. New harness modules live under `src/` (the harness), which may have dependencies — but this plan adds **no** new crate dependency (`std` only).
- **ONE SCORING PATH (Invariant 2):** scoring still goes through `ssi_scoring::score` in the parent; the worker only produces a permutation. On a *passing* submission the score is byte-identical to today.
- **CLOSED-FORM TESTS ALWAYS PASS (Invariant 4):** the scorer tests in `ssi-scoring/src/lib.rs` and the existing `tests/*.rs` stay green.
- **GREEN AND COMMITTED (Invariant 5):** `cargo test` passes at the end of every task; commit at each working milestone.
- **GIT HYGIENE:** the working tree contains pre-existing, unrelated changes (`D CLAUDE.md`, untracked `CLAUDE.md.bak`, `.claude/settings.json.bak`). **Never `git add -A` / `git add -u`.** Stage only the explicit paths each task names.

---

## File Structure

- `src/perm_io.rs` — **new.** Binary permutation read/write + bijection validation. Ported verbatim from `grader/runner/src/perm_io.rs`. One responsibility: serialize a `Vec<usize>` permutation to/from a file the parent and worker exchange.
- `src/watchdog.rs` — **new.** Spawn a child `Command`, poll it, SIGKILL on time breach; return a typed outcome. `std`-only, command-agnostic (unit-testable with `/bin/sleep`). One responsibility: time-bounded child supervision.
- `src/main.rs` — **modified.** Add mode dispatch (`--worker` vs parent); add a `worker` function; replace the in-process `order()` call + post-hoc cap check (lines ~92–124) with watchdog-supervised subprocess calls; update `TIME_CAP_PER_MATRIX` to 2 s; explicit error messages.
- `src/pattern.rs` — **modified (small).** Expose the corpus as `(line_index, source, Pattern)` so the parent can pass the worker the exact raw line index it scored (index-space agreement).
- `src/ordering/mod.rs` — **modified (one line, test-only).** An env-gated sleep hook (`SSI_TEST_SLEEP_MS`) at the top of `order()` so a test can force a cap breach without a real slow matrix.
- `README.md`, `docs/HARNESS-DESIGN.md` — **modified.** Document the 2 s cap, subprocess enforcement, and the local-stricter-than-grader note.

**Out of scope:** memory cap, grader changes, a shared runner crate (see spec §"Out of scope").

---

## Task 1: Port the binary permutation I/O module

**Files:**
- Create: `src/perm_io.rs`
- Modify: `src/main.rs` (add `mod perm_io;`)

**Interfaces:**
- Produces:
  - `pub fn write_perm(path: &Path, perm: &[usize]) -> std::io::Result<()>`
  - `pub fn read_perm(path: &Path) -> std::io::Result<Vec<usize>>`
  - Format: little-endian `u64` count, then `count` little-endian `u64` indices.

- [ ] **Step 1: Create `src/perm_io.rs` with the implementation and tests**

```rust
//! Binary permutation I/O — the parent↔worker exchange format for the
//! subprocess-enforced time cap. Ported from the grader's perm_io so the local
//! harness uses the same wire format. Trusted harness code (not scoring, not
//! submission code). `std`-only.
//! Format: little-endian u64 count, then `count` little-endian u64 indices.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub fn write_perm(path: &Path, perm: &[usize]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(8 + perm.len() * 8);
    buf.extend_from_slice(&(perm.len() as u64).to_le_bytes());
    for &p in perm {
        buf.extend_from_slice(&(p as u64).to_le_bytes());
    }
    let mut f = fs::File::create(path)?;
    f.write_all(&buf)
}

pub fn read_perm(path: &Path) -> std::io::Result<Vec<usize>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.len() < 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "perm file too short",
        ));
    }
    let count = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let expected = count.checked_mul(8).and_then(|m| m.checked_add(8));
    if expected != Some(bytes.len()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("perm file length mismatch: header says {count}"),
        ));
    }
    let mut perm = Vec::with_capacity(count);
    for i in 0..count {
        let off = 8 + i * 8;
        perm.push(u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap()) as usize);
    }
    Ok(perm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_perm() {
        let dir = std::env::temp_dir().join("ssi-harness-perm-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.bin");
        let perm = vec![3usize, 1, 0, 2];
        write_perm(&path, &perm).unwrap();
        assert_eq!(read_perm(&path).unwrap(), perm);
    }

    #[test]
    fn read_rejects_truncated() {
        let dir = std::env::temp_dir().join("ssi-harness-perm-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.bin");
        std::fs::write(&path, [1u8, 2, 3]).unwrap(); // < 8 bytes
        assert!(read_perm(&path).is_err());
    }

    #[test]
    fn read_rejects_overflow_count() {
        let dir = std::env::temp_dir().join("ssi-harness-perm-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("overflow.bin");
        std::fs::write(&path, u64::MAX.to_le_bytes()).unwrap();
        assert!(read_perm(&path).is_err());
    }
}
```

- [ ] **Step 2: Register the module in `src/main.rs`**

At the top of `src/main.rs`, the module declarations are:

```rust
mod ordering;
mod pattern;
mod purity;
```

Add `perm_io` and (for the next task) `watchdog`:

```rust
mod ordering;
mod pattern;
mod perm_io;
mod purity;
mod watchdog;
```

> Note: `mod watchdog;` will not compile until Task 2 creates the file. To keep this task green on its own, add only `mod perm_io;` now and add `mod watchdog;` in Task 2 Step 2.

So for THIS task add exactly:

```rust
mod ordering;
mod pattern;
mod perm_io;
mod purity;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --bin ssi-ordering-challenge perm_io`
Expected: PASS — `roundtrip_perm`, `read_rejects_truncated`, `read_rejects_overflow_count`.

- [ ] **Step 4: Commit**

```bash
git add src/perm_io.rs src/main.rs
git commit -m "harness: add binary permutation I/O (parent<->worker exchange format)"
```

---

## Task 2: Port the time-bounded watchdog

**Files:**
- Create: `src/watchdog.rs`
- Modify: `src/main.rs` (add `mod watchdog;`)

**Interfaces:**
- Produces:
  - `pub struct CapConfig { pub time_cap: Duration, pub poll: Duration }` with `Default` (time_cap 2 s, poll 10 ms).
  - `pub enum WorkerOutcome { Ok, Timeout, Crashed(String) }`
  - `pub fn run_capped(cmd: &mut std::process::Command, cfg: &CapConfig) -> WorkerOutcome` — spawn `cmd`, poll until exit or `time_cap`; on breach, kill the child and return `Timeout`; non-zero exit → `Crashed`; clean exit → `Ok`.

- [ ] **Step 1: Write `src/watchdog.rs` with the implementation and tests**

```rust
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
```

- [ ] **Step 2: Register the module in `src/main.rs`**

Update the module declarations to add `watchdog`:

```rust
mod ordering;
mod pattern;
mod perm_io;
mod purity;
mod watchdog;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --bin ssi-ordering-challenge watchdog`
Expected: PASS — `fast_command_is_ok`, `slow_command_times_out_promptly`, `nonzero_exit_is_crashed`.

- [ ] **Step 4: Commit**

```bash
git add src/watchdog.rs src/main.rs
git commit -m "harness: add std-only time-bounded watchdog (spawn/poll/SIGKILL at cap)"
```

---

## Task 3: Expose corpus line indices for index-space agreement

**Files:**
- Modify: `src/pattern.rs`

**Interfaces:**
- Consumes: `ssi_scoring::load_corpus_jsonl` (already returns `Vec<(String, Pattern)>` in file order).
- Produces: `pub fn dev_corpus_indexed() -> Vec<(usize, String, Pattern)>` — same entries as `dev_corpus()`, each tagged with its **0-based raw line index** in `patterns.jsonl`, so the parent can pass the worker the exact index `load_pattern_jsonl_line` will use.

Background: `load_corpus_jsonl` skips blank lines, while the worker's
`load_pattern_jsonl_line(path, i)` counts raw lines. The shipped corpus has no
blank lines, but to make correctness independent of that the parent must hand
the worker the *raw* line index of each scored entry. We compute that index by
re-deriving it the same way `load_corpus_jsonl` enumerates lines.

- [ ] **Step 1: Read the current `src/pattern.rs`**

Confirm it currently exposes `pub const DEV_CORPUS_FILE: &str` and
`pub fn dev_corpus() -> Vec<(String, Pattern)>`.

- [ ] **Step 2: Add `dev_corpus_indexed()` and have `dev_corpus()` delegate to it**

Replace the body of `dev_corpus()` and add the indexed variant. The raw line
index is derived by enumerating the file's lines and skipping blanks — the
identical rule `load_corpus_jsonl` uses — and pairing surviving lines with the
parsed corpus in order:

```rust
/// Load the dev corpus tagged with each entry's 0-based RAW line index in
/// `patterns.jsonl` (blank lines counted), so the parent can hand the worker the
/// exact index `ssi_scoring::load_pattern_jsonl_line` will resolve. Parsing
/// still goes through the shared `ssi_scoring::load_corpus_jsonl` reader; this
/// only recovers the raw indices of the non-blank lines it kept.
pub fn dev_corpus_indexed() -> Vec<(usize, String, Pattern)> {
    let path = PathBuf::from(DEV_CORPUS_FILE);
    let corpus = match ssi_scoring::load_corpus_jsonl(&path) {
        Ok(c) => c,
        Err(e) => {
            if path.exists() {
                panic!("failed to load {}: {e}", path.display());
            }
            return Vec::new();
        }
    };
    // Recover the raw line index of each non-blank line, in file order — the
    // same lines load_corpus_jsonl parsed, in the same order.
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let raw_indices: Vec<usize> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, _)| i)
        .collect();
    debug_assert_eq!(raw_indices.len(), corpus.len(), "index/corpus length mismatch");
    corpus
        .into_iter()
        .zip(raw_indices)
        .map(|((source, pat), idx)| (idx, source, pat))
        .collect()
}

/// Load the shipped development corpus: every pattern in
/// `corpus/dev/patterns.jsonl`, in file order, named by its `source` problem.
pub fn dev_corpus() -> Vec<(String, Pattern)> {
    dev_corpus_indexed()
        .into_iter()
        .map(|(_, source, pat)| (source, pat))
        .collect()
}
```

Keep the existing `use std::path::PathBuf;` and `pub use ssi_scoring::Pattern;`
and `pub const DEV_CORPUS_FILE`.

- [ ] **Step 3: Add a test asserting index-space agreement**

Append to `src/pattern.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_corpus_matches_single_line_loader() {
        // Each (raw_index, _, pattern) from dev_corpus_indexed must equal the
        // pattern load_pattern_jsonl_line resolves for that raw index — proving
        // the parent and worker agree on which matrix an index names.
        let path = std::path::Path::new(DEV_CORPUS_FILE);
        for (idx, _src, pat) in dev_corpus_indexed() {
            let one = ssi_scoring::load_pattern_jsonl_line(path, idx)
                .expect("worker loader resolves the raw index");
            assert_eq!(one.n, pat.n, "n mismatch at raw line {idx}");
            assert_eq!(one.col_ptr, pat.col_ptr, "col_ptr mismatch at raw line {idx}");
            assert_eq!(one.row_idx, pat.row_idx, "row_idx mismatch at raw line {idx}");
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --bin ssi-ordering-challenge indexed_corpus_matches_single_line_loader`
Expected: PASS (the parent index and the worker loader resolve the same `Pattern` for every entry).

- [ ] **Step 5: Commit**

```bash
git add src/pattern.rs
git commit -m "harness: expose dev_corpus_indexed() with raw line indices (parent/worker agreement)"
```

---

## Task 4: Worker mode + parent subprocess enforcement in `main.rs`

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `perm_io::{write_perm, read_perm}`, `watchdog::{run_capped, CapConfig, WorkerOutcome}`, `pattern::dev_corpus_indexed`, `ssi_scoring::{load_pattern_jsonl_line, score, amd_baseline}`.
- Produces: a `--worker <jsonl_path> <line_index> <out_perm>` mode; a parent loop that scores via the capped subprocess.

This is the core task. It (a) adds mode dispatch, (b) adds a `worker` fn, (c) sets the cap to 2 s, and (d) replaces the in-process `order()` call + post-hoc cap check with two capped subprocess invocations.

- [ ] **Step 1: Change the cap constant to 2 s**

In `src/main.rs`, replace:

```rust
const TIME_CAP_PER_MATRIX: Duration = Duration::from_secs(5);
```

with:

```rust
/// Per-matrix time cap, ENFORCED: order() runs in a child process that is
/// SIGKILLed at this bound (see watchdog). 2 s is the strict end of the cost
/// doc's 2–5 s band; it is stricter than the grader's current 5 s default, so a
/// submission that passes locally passes the server gate.
const TIME_CAP_PER_MATRIX: Duration = Duration::from_secs(2);
```

- [ ] **Step 2: Add mode dispatch at the top of `main()`**

`main()` currently begins (around `src/main.rs:56`):

```rust
fn main() {
    let note = parse_note();
```

Insert worker-mode dispatch as the very first lines of `main()`:

```rust
fn main() {
    // Worker mode: the ONLY process that runs contestant order(). Loads one
    // pattern by raw line index, runs order(), writes the permutation, exits.
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.get(1).map(String::as_str) == Some("--worker") {
        std::process::exit(worker(&raw_args[2..]));
    }

    let note = parse_note();
```

- [ ] **Step 3: Add the `worker` function**

Add this function to `src/main.rs` (e.g. just after `main()`):

```rust
/// `--worker <jsonl_path> <line_index> <out_perm>`: load one pattern, run the
/// contestant order(), write the permutation. The parent supervises this under
/// the time cap and SIGKILLs it on breach. A panic aborts the process (non-zero
/// exit, no perm file) — the parent treats that as a FAIL.
fn worker(args: &[String]) -> i32 {
    let (Some(jsonl), Some(idx_s), Some(out)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("--worker: usage: --worker <jsonl_path> <line_index> <out_perm>");
        return 2;
    };
    let Ok(line_index) = idx_s.parse::<usize>() else {
        eprintln!("--worker: line_index must be a non-negative integer");
        return 2;
    };
    let pat = match ssi_scoring::load_pattern_jsonl_line(Path::new(jsonl), line_index) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("worker: failed to load pattern {line_index} from {jsonl}: {e}");
            return 3;
        }
    };
    let perm = ordering::order(&pat);
    match perm_io::write_perm(Path::new(out), &perm) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("worker: failed to write perm to {out}: {e}");
            4
        }
    }
}
```

- [ ] **Step 4: Replace the corpus load + per-matrix loop**

The parent currently does `let corpus = pattern::dev_corpus();` (~line 68) and a
loop `for (name, pat) in &corpus {` (~line 87) that calls `ordering::order(pat)`
in-process. Change the corpus load to the indexed variant:

```rust
    let corpus = pattern::dev_corpus_indexed();
    if corpus.is_empty() {
        println!(
            "RUN FAILED: no patterns found at {}. Run from the repo root.",
            pattern::DEV_CORPUS_FILE
        );
        std::process::exit(1);
    }
```

Then replace the body of the per-matrix loop. The new loop header is
`for (line_index, name, pat) in &corpus {`. Replace the section from the AMD
baseline through the determinism/validation gates (currently lines ~88–124)
with:

```rust
    // Resolve our own executable + a scratch dir for worker perm files.
    let exe = std::env::current_exe().expect("locate harness executable");
    let scratch = std::env::temp_dir().join(format!("ssi-harness-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let jsonl_path = pattern::DEV_CORPUS_FILE.to_string();
    let cap = watchdog::CapConfig { time_cap: TIME_CAP_PER_MATRIX, poll: std::time::Duration::from_millis(10) };

    for (line_index, name, pat) in &corpus {
        // --- AMD baseline (trusted, in-process) ---
        let base_perm = ssi_scoring::amd_baseline(pat);
        let base = score(pat, &base_perm);

        // --- contestant ordering: capped subprocess, run twice ---
        let run_once = |tag: &str| -> Result<Vec<usize>, String> {
            let out_perm = scratch.join(format!("{line_index}-{tag}.bin"));
            let _ = std::fs::remove_file(&out_perm);
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--worker").arg(&jsonl_path).arg(line_index.to_string()).arg(&out_perm);
            let t0 = Instant::now();
            match watchdog::run_capped(&mut cmd, &cap) {
                watchdog::WorkerOutcome::Ok => perm_io::read_perm(&out_perm)
                    .map_err(|e| format!("worker produced no readable permutation: {e}")),
                watchdog::WorkerOutcome::Timeout => Err(format!(
                    "order() exceeded the {:.1}s per-matrix cap and was killed (took ≥ {:.1}s). \
                     Your ordering must return within {:.0}s on every matrix. This matrix is \
                     n={}, nnz={}, nnz/n≈{} — if it is dense, the cost is in order() itself; \
                     gate expensive paths by BOTH n and nnz.",
                    TIME_CAP_PER_MATRIX.as_secs_f64(),
                    t0.elapsed().as_secs_f64(),
                    TIME_CAP_PER_MATRIX.as_secs_f64(),
                    pat.n, pat.nnz(), pat.nnz() / pat.n.max(1)
                )),
                watchdog::WorkerOutcome::Crashed(why) => Err(format!("order() crashed: {why}")),
            }
        };

        let perm1 = match run_once("a") {
            Ok(p) => p,
            Err(e) => { failed = Some(format!("{name}: {e}")); break; }
        };
        let perm2 = match run_once("b") {
            Ok(p) => p,
            Err(e) => { failed = Some(format!("{name}: {e}")); break; }
        };
        if perm1 != perm2 {
            failed = Some(format!("{name}: nondeterministic ordering (two runs differ)"));
            break;
        }
        if let Err(e) = validate_permutation(&perm1, pat.n) {
            failed = Some(format!("{name}: invalid permutation — {e}"));
            break;
        }
```

Leave the rest of the loop body (trusted scoring at ~line 127 onward —
`let yours = score(pat, &perm1);` through the per-matrix print) unchanged. The
variable `elapsed` used in the old per-matrix print no longer exists; replace
the per-matrix print's `elapsed.as_secs_f64() * 1e3` time column with the
parent-observed time. To keep it simple and avoid threading timing out of the
closure, change the printed line's time field to display `"-"`:

In the `format!` that builds `line` (the per-matrix table row), replace the
trailing time argument so the format no longer references `elapsed`. Change:

```rust
        let line = format!(
            "{:<28} {:>8} {:>10} {:>14} {:>14} {:>8.3} {:>7.0}ms",
            name, pat.n, pat.nnz(), base.flops, yours.flops, ratio,
            elapsed.as_secs_f64() * 1e3
        );
```

to:

```rust
        let line = format!(
            "{:<28} {:>8} {:>10} {:>14} {:>14} {:>8.3} {:>9}",
            name, pat.n, pat.nnz(), base.flops, yours.flops, ratio, "(capped)"
        );
```

- [ ] **Step 5: Clean up the scratch dir before the final emit**

After the loop, before the `match failed { ... }` block, add:

```rust
    let _ = std::fs::remove_dir_all(&scratch);
```

- [ ] **Step 6: Build and smoke-test on the sample corpus**

Run: `cargo build --release`
Expected: compiles clean.

Run: `cargo run --release -- --note "subprocess cap smoke test"`
Expected: prints the 13-row per-matrix table (time column shows `(capped)`), a
finite score line (≈ 0.969 on the sample), writes `score.json`, appends an `OK`
row to `results.tsv`. No FAIL.

> If `results.tsv` gains a smoke-test row you do not want committed, revert it
> with `git checkout results.tsv` before the commit (do not stage it).

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "harness: enforce 2s ordering cap via SIGKILLed worker subprocess"
```

---

## Task 5: End-to-end cap-enforcement test (env-gated slow hook)

**Files:**
- Modify: `src/ordering/mod.rs` (one env-gated sleep at the top of `order()`)
- Create: `tests/time_cap.rs`

**Interfaces:**
- Consumes: the `--worker` mode and the parent cap loop from Task 4.
- Produces: a regression test proving (a) a too-slow `order()` is killed and the run FAILs promptly, and (b) a normal run over the sample corpus still succeeds.

The hook lets a test force a breach deterministically without a real slow
matrix. It is env-gated so it is inert in every normal run and on the grader.

- [ ] **Step 1: Add the env-gated sleep hook at the top of `order()`**

In `src/ordering/mod.rs`, `order()` currently begins:

```rust
pub fn order(pattern: &Pattern) -> Vec<usize> {
    let n = pattern.n;
    if n == 0 {
        return vec![];
    }
```

Insert the hook as the first statements of `order()`:

```rust
pub fn order(pattern: &Pattern) -> Vec<usize> {
    // TEST-ONLY hook: when SSI_TEST_SLEEP_MS is set, sleep that long before
    // ordering. Inert unless the env var is present (never set in normal runs
    // or on the grader); lets the harness's time-cap test force a breach.
    if let Ok(ms) = std::env::var("SSI_TEST_SLEEP_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }

    let n = pattern.n;
    if n == 0 {
        return vec![];
    }
```

> This is the ONLY permitted change to `src/ordering/` in this plan. It adds no
> dependency (stdlib `std::env` / `std::thread`), so the purity gate still
> passes.

- [ ] **Step 2: Write the end-to-end test**

Create `tests/time_cap.rs`:

```rust
//! End-to-end: the harness enforces the per-matrix time cap by killing a slow
//! order() worker, and a normal run over the sample corpus still succeeds.
//! Drives the release binary the way a contestant does.

use std::process::Command;
use std::time::{Duration, Instant};

fn harness_bin() -> std::path::PathBuf {
    // The integration test binary runs from target/<profile>/deps; the harness
    // binary is two levels up at target/<profile>/ssi-ordering-challenge.
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // deps
    p.pop(); // profile dir
    p.push("ssi-ordering-challenge");
    p
}

#[test]
fn normal_run_succeeds_on_sample_corpus() {
    let out = Command::new(harness_bin())
        .args(["--note", "time_cap test: normal"])
        .output()
        .expect("run harness");
    assert!(
        out.status.success(),
        "expected success, got {:?}\nstdout:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn slow_ordering_is_killed_and_fails_promptly() {
    // Force every order() call to sleep 30s; the 2s cap must kill it and FAIL
    // the run in well under 30s.
    let start = Instant::now();
    let out = Command::new(harness_bin())
        .env("SSI_TEST_SLEEP_MS", "30000")
        .args(["--note", "time_cap test: slow"])
        .output()
        .expect("run harness");
    let elapsed = start.elapsed();

    assert!(!out.status.success(), "slow ordering should FAIL the run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("time cap")
            || stdout.contains("per-matrix cap")
            || stdout.contains("RUN FAILED"),
        "expected a time-cap failure message, got:\n{stdout}"
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "cap was not enforced promptly: {elapsed:?}"
    );
}
```

> The error text asserted (`per-matrix cap`) matches the message produced in
> Task 4 Step 4. If you changed that wording, update the assertion to match.

- [ ] **Step 3: Build the release binary the test drives**

Run: `cargo build --release`
Expected: compiles clean (the test invokes the built binary).

- [ ] **Step 4: Run the test**

Run: `cargo test --release --test time_cap`
Expected: PASS — `normal_run_succeeds_on_sample_corpus` and
`slow_ordering_is_killed_and_fails_promptly` (the latter finishing in a few
seconds, not 30).

> Use `--release` so `current_exe`-relative resolution points at
> `target/release/ssi-ordering-challenge`. If you run without `--release`, build
> and test both in debug instead so the paths line up.

- [ ] **Step 5: Run the full suite**

Run: `cargo test --release`
Expected: all green — new `time_cap` tests, `perm_io`, `watchdog`, the
`pattern` index test, the closed-form scorer tests, and the existing
`exact_equivalence` / `narrow_input` / `scorer_crosscheck` tests.

- [ ] **Step 6: Commit**

```bash
git add src/ordering/mod.rs tests/time_cap.rs
git commit -m "test: end-to-end time-cap enforcement (env-gated slow hook) + normal-run regression"
```

---

## Task 6: Document the enforced 2 s cap

**Files:**
- Modify: `README.md`
- Modify: `docs/HARNESS-DESIGN.md`

**Interfaces:** none (documentation only).

- [ ] **Step 1: Update the time-cap rule in `README.md`**

In `README.md`, the validity section currently says:

```
- **Time cap.** 5 s per matrix (annealing and learned orderings are welcome;
  runaways are not).
```

Replace with:

```
- **Time cap.** 2 s per matrix, **enforced**: `order()` runs in a child process
  that is killed the instant it exceeds the cap, and the run FAILs with the
  offending matrix's size/density. (This is the strict end of the proposal's
  2–5 s band and is stricter than the server's current default, so an ordering
  that passes locally passes the server gate.) Annealing and learned orderings
  are welcome; runaways are not — and note that cost scales with **density
  (nnz)**, not just dimension.
```

- [ ] **Step 2: Reconcile any other "5 s" mention in `README.md`**

Run: `rg "5 ?s|5 second" README.md`
For each hit referring to the per-matrix cap, change `5 s` → `2 s`. (Leave
unrelated numbers alone.)

- [ ] **Step 3: Update `docs/HARNESS-DESIGN.md` Stage B description**

In `docs/HARNESS-DESIGN.md`, the Stage B row currently reads:

```
| B — sandboxed compile & run | plain `cargo run` here, with the 5 s/matrix **time cap** enforced in-process; the production grader adds a no-network/no-filesystem sandbox and a 2–4 GB memory cap |
```

Replace with:

```
| B — sandboxed compile & run | `cargo run` here runs each `order()` in a child process (`--worker` mode) supervised by a watchdog (`src/watchdog.rs`) that SIGKILLs it at the **2 s/matrix time cap** — the same enforcement mechanism the grader uses; the production grader additionally adds a no-network/no-filesystem sandbox and a 2–4 GB memory cap |
```

- [ ] **Step 4: Verify and commit**

Run: `rg "5 ?s" README.md docs/HARNESS-DESIGN.md` and confirm no stale
per-matrix-cap references remain.

```bash
git add README.md docs/HARNESS-DESIGN.md
git commit -m "docs: document the enforced 2s subprocess ordering cap"
```

---

## Self-Review

**Spec coverage:**
- Subprocess + SIGKILL enforcement → Tasks 2 (watchdog) + 4 (worker mode + parent loop). ✓
- Worker mode loads via `load_pattern_jsonl_line` → Task 4 Step 3. ✓
- Parent computes baseline/score in-process, runs worker twice (determinism) → Task 4 Step 4. ✓
- New `src/watchdog.rs` + `src/perm_io.rs`, `std`-only → Tasks 1, 2. ✓
- Index-space agreement (parent passes raw line index) → Task 3 + its test. ✓
- Flat 2 s cap, replacing 5 s → Task 4 Step 1. ✓
- Explicit, actionable Stage-B error with n/nnz/nnz·n⁻¹ → Task 4 Step 4. ✓
- Worker crash distinct from timeout → Task 4 Step 4 (`Crashed` vs `Timeout`). ✓
- Frozen contract (signature/score/formats unchanged; cap is a parameter) → no task changes them; verified in Task 4. ✓
- Tests: watchdog, perm_io, index agreement, end-to-end cap + normal run → Tasks 1, 2, 3, 5. ✓
- Docs incl. local-stricter-than-grader note → Task 6. ✓
- Out of scope (memory cap, grader, shared crate) → not present in any task. ✓

**Placeholder scan:** No TBD/TODO/"handle errors appropriately". All code blocks complete. Error message text is concrete and matched by the Task 5 assertion. ✓

**Type consistency:** `WorkerOutcome::{Ok, Timeout, Crashed(String)}`, `CapConfig{time_cap, poll}`, `run_capped(&mut Command, &CapConfig) -> WorkerOutcome` consistent across Tasks 2 and 4. `dev_corpus_indexed() -> Vec<(usize, String, Pattern)>` defined Task 3, consumed Task 4. `write_perm`/`read_perm` defined Task 1, consumed Task 4. `worker(&[String]) -> i32` defined and dispatched Task 4. ✓

**Git hygiene:** every commit step stages explicit paths; no `git add -A`. The pre-existing `CLAUDE.md`/`.bak` changes are never staged. ✓

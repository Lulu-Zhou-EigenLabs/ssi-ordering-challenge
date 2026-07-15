# Trusted-path panic guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A panic in the trusted in-process baseline/score path becomes a recorded `FAIL` row (with reason) in `results.tsv` and always disposes the scratch dir, instead of aborting the harness and leaking temp files (issue #19).

**Architecture:** Extract three small, unit-testable seams into a new `src/failsafe.rs` module — a `catch_unwind` wrapper, a panic-message extractor, a FAIL-note composer, and a `ScratchDir` Drop guard. Wire them into `src/main.rs`: guard the trusted baseline+score closures, thread the failure reason into the `results.tsv` note column, and return `ExitCode` (instead of `std::process::exit`) with the scratch dir owned by the Drop guard so cleanup runs on every path.

**Tech Stack:** Rust (stdlib only in the harness for these seams: `std::panic`, `std::process::ExitCode`, `std::fs`).

## Global Constraints

- **Invariant 1 — contract frozen:** do NOT change the `order()` signature, the score definition, the gates, or the `score.json`/`results.tsv` output *formats*. `results.tsv` stays exactly 5 tab-separated columns: `ts \t status \t score \t fill \t note`. Changing *note content* is allowed; changing columns is not.
- **Invariant 2 — one scoring path:** the grader runs this same harness binary (`cargo run --release`), so no separate grader change; do not copy/re-implement scoring.
- **Invariant 3 — submission dir stays stdlib-only:** all changes here are in trusted harness code (`src/main.rs`, `src/failsafe.rs`); do NOT touch `src/ordering/` and add no `[dependencies]`.
- **Invariant 4 — closed-form tests always pass.**
- **Invariant 5 — green and committed:** `cargo test` passes at the end of every task; commit each working milestone.
- `score.json` is written ONLY on OK (unchanged). A FAIL writes only a `results.tsv` row.
- `amd_baseline`/`score` signatures in `ssi-scoring` stay unchanged (panic-on-error); the harness catches.

---

### Task 1: `failsafe` module — panic catch, message extraction, note composition, scratch guard

**Files:**
- Create: `src/failsafe.rs`
- Modify: `src/main.rs` (add `mod failsafe;` near the other `mod` declarations at lines 44-48)
- Test: unit tests inside `src/failsafe.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing (stdlib only).
- Produces, for Task 2 to use:
  - `pub fn catch<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T, String>` — runs `f`, returns `Ok(value)` or `Err(panic_message)`.
  - `pub fn compose_note(reason: &str, user_note: &str) -> String` — `"<reason> | <user_note>"` when `user_note` non-empty, else `"<reason>"`; strips `\t`/`\n` from both.
  - `pub struct ScratchDir(pub std::path::PathBuf)` — removes the dir on `Drop`. `ScratchDir::path(&self) -> &std::path::Path` accessor.

- [ ] **Step 1: Write the failing tests**

Create `src/failsafe.rs` with the test module first (implementation stubs added in Step 3):

```rust
//! Failure-containment seams for the trusted harness path (issue #19).
//!
//! The per-matrix AMD baseline and scoring run in-process in the parent. A
//! panic there (feral internal error, or an i32-overflow-sized pattern) must
//! become a recorded FAIL — not a process abort that leaks the scratch dir.
//! These are trusted harness helpers, kept in one small module so each seam is
//! unit-testable without spawning the whole binary.

use std::any::Any;
use std::path::{Path, PathBuf};

/// Run a trusted-path closure, converting a panic into `Err(message)`. Mirrors
/// the containment the contestant `order()` path already gets from its
/// subprocess+watchdog, for the in-process baseline/score path.
pub fn catch<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T, String> {
    std::panic::catch_unwind(f).map_err(panic_message)
}

/// Best-effort extraction of a panic payload's message.
fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic (non-string payload)".to_string()
    }
}

/// Strip tab/newline so a value is safe for a single TSV field.
fn sanitize(s: &str) -> String {
    s.replace(['\t', '\n'], " ")
}

/// Compose the `results.tsv` note column for a FAIL row: the failure reason,
/// then ` | ` + the user's note when one was given. Both are TSV-sanitized.
pub fn compose_note(reason: &str, user_note: &str) -> String {
    let reason = sanitize(reason);
    if user_note.is_empty() {
        reason
    } else {
        format!("{reason} | {}", sanitize(user_note))
    }
}

/// Owns the harness scratch dir and removes it on drop, so cleanup runs on
/// every exit path (OK, FAIL, or a stray unwind) once `main` stops calling
/// `std::process::exit`.
pub struct ScratchDir(pub PathBuf);

impl ScratchDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_returns_ok_for_non_panicking_closure() {
        assert_eq!(catch(|| 21 * 2), Ok(42));
    }

    #[test]
    fn catch_converts_str_panic_to_err_and_survives() {
        let r = catch(|| -> i32 { panic!("boom") });
        assert_eq!(r, Err("boom".to_string()));
        // reaching here proves the process was not aborted
    }

    #[test]
    fn catch_converts_string_panic_to_err() {
        let r = catch(|| -> i32 { panic!("{}", format!("code {}", 7)) });
        assert_eq!(r, Err("code 7".to_string()));
    }

    #[test]
    fn panic_message_handles_non_string_payload() {
        let payload: Box<dyn Any + Send> = Box::new(123_u32);
        assert_eq!(panic_message(payload), "unknown panic (non-string payload)");
    }

    #[test]
    fn compose_note_without_user_note_is_reason_only() {
        assert_eq!(compose_note("matrix too large", ""), "matrix too large");
    }

    #[test]
    fn compose_note_with_user_note_joins_with_pipe() {
        assert_eq!(
            compose_note("matrix too large", "retry knob"),
            "matrix too large | retry knob"
        );
    }

    #[test]
    fn compose_note_strips_tabs_and_newlines() {
        assert_eq!(
            compose_note("a\tb", "c\nd"),
            "a b | c d"
        );
    }

    #[test]
    fn scratch_dir_removed_on_drop() {
        let dir = std::env::temp_dir().join("ssi-failsafe-scratch-test");
        std::fs::create_dir_all(&dir).unwrap();
        {
            let _guard = ScratchDir(dir.clone());
            assert!(dir.exists());
        }
        assert!(!dir.exists());
    }

    #[test]
    fn scratch_dir_drop_is_noop_when_missing() {
        let dir = std::env::temp_dir().join("ssi-failsafe-scratch-missing");
        let _ = std::fs::remove_dir_all(&dir);
        // dropping a guard over a non-existent dir must not panic
        drop(ScratchDir(dir));
    }
}
```

- [ ] **Step 2: Add `mod failsafe;` and run tests to verify they compile+pass**

In `src/main.rs`, add `mod failsafe;` alphabetically among the existing module declarations (lines 44-48: `mod corpus; mod ordering; mod perm_io; mod purity; mod watchdog;`), so it reads `mod corpus; mod failsafe; mod ordering; ...`.

Run: `cargo test --bin ssi-ordering-challenge failsafe`
Expected: the 9 `failsafe::tests::*` tests PASS. (Implementation is already inline above; this task's test and impl ship together since the seams are tiny and interdependent.)

- [ ] **Step 3: Verify the whole suite is still green**

Run: `cargo test`
Expected: PASS (no existing test references `failsafe`; the new module only adds tests).

- [ ] **Step 4: Commit**

```bash
git add src/failsafe.rs src/main.rs
git commit -m "feat(harness): add failsafe seams — catch_unwind, note composer, scratch Drop guard

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Wire failsafe into `main` — guard trusted path, record reason, return ExitCode

**Files:**
- Modify: `src/main.rs` (imports; `main()` signature and body: scratch creation ~line 117, the per-matrix trusted baseline/score at lines 126-128 and 172, the loop-end cleanup line 194, and the final `match failed` block lines 198-267; `append_results` is reused as-is)

**Interfaces:**
- Consumes from Task 1: `failsafe::catch`, `failsafe::compose_note`, `failsafe::ScratchDir`.
- Produces: `fn main() -> std::process::ExitCode`; behavior — trusted-path panic → `FAIL` row whose note is `compose_note(reason, user_note)`; scratch dir removed on every return.

- [ ] **Step 1: Change `main` to return `ExitCode` and own scratch via the guard**

At the top of `src/main.rs`, add to the `use` block (near line 54):

```rust
use std::process::ExitCode;
```

Change the signature `fn main() {` to `fn main() -> ExitCode {`.

Replace the worker-dispatch early exit (lines 80-83) — it may keep using `std::process::exit` because it runs before any scratch dir exists:

```rust
    if raw_args.get(1).map(String::as_str) == Some("--worker") {
        return ExitCode::from(worker(&raw_args[2..]) as u8);
    }
```

Replace the Stage A purity early-return (lines 89-94):

```rust
    if let Err(e) = purity::check(&repo_root) {
        println!("RUN FAILED (Stage A — purity/license): {e}");
        let ts = now();
        append_results(ts, "FAIL", f64::NAN, f64::NAN, &note);
        return ExitCode::FAILURE;
    }
```

Replace the empty-corpus early-return (lines 98-104):

```rust
    if corpus.is_empty() {
        println!(
            "RUN FAILED: no patterns found at {}. Run from the repo root.",
            corpus_file.display()
        );
        return ExitCode::FAILURE;
    }
```

Replace the scratch creation (lines 117-118) so the dir is owned by the guard:

```rust
    let scratch = failsafe::ScratchDir(
        std::env::temp_dir().join(format!("ssi-harness-{}", std::process::id())),
    );
    std::fs::create_dir_all(scratch.path()).expect("create scratch dir");
```

Every later use of `scratch.join(...)` becomes `scratch.path().join(...)` (the two `run_once` closure lines around 132).

Delete the explicit loop-end cleanup (line 194: `let _ = std::fs::remove_dir_all(&scratch);`) — the guard now owns disposal.

- [ ] **Step 2: Guard the trusted baseline+score with `catch`**

Replace the per-matrix trusted baseline/score (lines 126-128):

```rust
        // --- AMD baseline (trusted, in-process) — guarded so a feral panic
        // or an i32-overflow-sized pattern becomes a recorded FAIL, not a
        // process abort that leaks scratch (issue #19). ---
        let base = match failsafe::catch(std::panic::AssertUnwindSafe(|| {
            let base_perm = ssi_scoring::amd_baseline(pat);
            score(pat, &base_perm)
        })) {
            Ok(b) => b,
            Err(msg) => {
                failed = Some(format!("{name}: trusted baseline/score panicked — {msg}"));
                break;
            }
        };
```

Note: `&Pattern` references captured by the closure are not `UnwindSafe`, so wrap the closure in `std::panic::AssertUnwindSafe` (sound here — a caught panic does not leave the read-only borrowed pattern observably broken).

Replace the contestant-permutation scoring (line 172) with a guarded version:

```rust
        // --- trusted scoring (Stage D), same path as the grader — guarded ---
        let yours = match failsafe::catch(std::panic::AssertUnwindSafe(|| score(pat, &perm1))) {
            Ok(s) => s,
            Err(msg) => {
                failed = Some(format!("{name}: trusted scoring panicked — {msg}"));
                break;
            }
        };
```

- [ ] **Step 3: Record the reason in the FAIL note; return ExitCode from the match**

Replace the final `match failed { ... }` block (lines 198-267). The `Some(reason)` arm composes the note and returns `ExitCode::FAILURE`; the `None` arm keeps its body verbatim but ends with `return ExitCode::SUCCESS`. Only the changed head/tail is shown; the `None`-arm body between them is unchanged from the current file:

```rust
    match failed {
        Some(reason) => {
            println!("\nRUN FAILED: {reason}");
            append_results(
                timestamp,
                "FAIL",
                f64::NAN,
                f64::NAN,
                &failsafe::compose_note(&reason, &note),
            );
            ExitCode::FAILURE
        }
        None => {
            // ... existing per-bucket geomean, breakdown table, score.json
            //     write, and append_results(timestamp, "OK", ...) body, VERBATIM ...
            append_results(timestamp, "OK", score_val, fill, &note);
            ExitCode::SUCCESS
        }
    }
```

(The `std::process::exit(1)` previously inside the `Some` arm is gone — returning `ExitCode::FAILURE` lets the `ScratchDir` guard drop and clean up.)

- [ ] **Step 4: Build and run the whole suite**

Run: `cargo test`
Expected: PASS — including `failsafe::tests::*` and the existing `tests/` integration suites (exact_equivalence, scorer_crosscheck, time_cap).

- [ ] **Step 5: End-to-end smoke — a normal OK run still works and cleans up**

Run: `cargo run --release -- --note "issue19 smoke"`
Expected: prints the per-matrix table + `score (…)`, appends an `OK` row to `results.tsv`, writes `score.json`, exits 0.

Verify no scratch leak:
Run: `ls "$TMPDIR" | grep ssi-harness || echo "no scratch leak"`
Expected: `no scratch leak`.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "fix(harness): guard trusted baseline/score with catch_unwind; record FAIL reason; always dispose scratch (#19)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Update harness doc comment and HARNESS-DESIGN note

**Files:**
- Modify: `src/main.rs` (the module doc comment, lines 40-42: the "Any invalid permutation, panic, …" paragraph)
- Modify: `docs/HARNESS-DESIGN.md` (the `results.tsv` line ~63)

**Interfaces:**
- Consumes: nothing.
- Produces: docs reflecting that trusted-path panics are now caught and recorded.

- [ ] **Step 1: Update the main.rs doc paragraph**

Replace the paragraph at lines 40-42:

```rust
//! Any invalid permutation, panic, nondeterminism, cap violation, or
//! purity/license failure makes the whole run FAIL — no partial credit, no
//! silent fallback. A panic in the trusted in-process baseline/score path
//! (feral internal error or an oversized pattern) is caught and recorded as a
//! FAIL row whose note carries the reason; the scratch dir is disposed on
//! every exit path.
```

- [ ] **Step 2: Update HARNESS-DESIGN.md**

Change the `results.tsv` description line (~63) to note the FAIL reason:

```
├── results.tsv           append-only run log (timestamp, status, score, fill, note; FAIL rows carry the reason in note)
```

- [ ] **Step 3: Verify still green + commit**

Run: `cargo test`
Expected: PASS.

```bash
git add src/main.rs docs/HARNESS-DESIGN.md
git commit -m "docs: note trusted-path panic handling and FAIL-reason recording (#19)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- catch_unwind guard on trusted baseline+score → Task 2 Step 2 (both the baseline+score closure and the contestant-perm score). ✓
- FAIL reason recorded in results.tsv note → Task 1 (`compose_note`) + Task 2 Step 3. ✓
- Scratch disposal on every exit (ExitCode + Drop guard) → Task 1 (`ScratchDir`) + Task 2 Steps 1&3. ✓
- Unit tests for panic_message / catch / note composition / ScratchDir → Task 1 Step 1. ✓
- Regression (existing tests, OK still writes score.json) → Task 2 Steps 4-5. ✓
- Out of scope respected: no ssi-scoring signature change, score.json only on OK, formats frozen. ✓

**Placeholder scan:** none — all code shown; the one "verbatim" reference (Task 2 Step 3 `None`-arm body) explicitly points at the current file's unchanged block rather than hiding new code.

**Type consistency:** `catch`, `compose_note`, `ScratchDir`/`ScratchDir::path` names and signatures match between Task 1 (definition) and Task 2 (use). `main -> ExitCode` consistent across Task 2 steps.

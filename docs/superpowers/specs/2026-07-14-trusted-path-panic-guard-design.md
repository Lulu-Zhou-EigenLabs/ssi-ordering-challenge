# Trusted baseline/score panic guard — design

**Issue:** #19 — *Trusted per-matrix baseline/score panics abort the whole run
instead of emitting a FAIL row.*

**Date:** 2026-07-14

## Problem

The per-matrix AMD baseline and scoring run **in-process in the parent
harness** with no panic guard:

```rust
// src/main.rs (in the per-matrix loop)
let base_perm = ssi_scoring::amd_baseline(pat);   // in-process, no catch
let base = score(pat, &base_perm);
```

`amd_baseline` panics via `.expect(...)` on two fallible steps
(`ssi-scoring/src/lib.rs:119,124,128`): an `i32::try_from` overflow on a
pathological/oversized pattern, and a feral-internal AMD error. `score` can
likewise panic on an internal feral error.

A panic in this trusted path unwinds the whole harness process. Because the
FAIL-row append (`append_results`) and the scratch-dir cleanup
(`remove_dir_all`) both live *after* the loop, a panic during the loop means:

- no `FAIL` row is appended to `results.tsv`,
- no artifact records *why* the run died (only a stdout backtrace), and
- the temp scratch dir (`$TMPDIR/ssi-harness-<pid>/`) **leaks**.

The contestant `order()` path is already guarded (subprocess + watchdog →
timeout/crash become a recorded FAIL). Only the trusted path is exposed.

Two secondary gaps surface while fixing this:

1. **FAIL rows don't record the reason.** Every FAIL — existing gates
   included — writes only the user's `--note` to `results.tsv`; the reason is
   printed to stdout and lost from the artifact.
2. **`std::process::exit(1)` skips destructors.** A plain RAII scratch guard
   would not fire on the FAIL path, so scratch disposal must not depend on
   normal unwinding alone.

## Decision: `catch_unwind`, not `Result`

In normal operation the baseline never fails. A baseline failure means
abnormal input or a bug — something a **human** investigates (remove the
matrix, fix the bug), not something the harness recovers from
programmatically. Given that:

- We need to catch **everything**, including unforeseen panics deep in feral,
  not just the overflow we anticipated. `catch_unwind` traps all panics;
  `Result` only surfaces errors we convert by hand and would still let an
  internal feral `unwrap` abort the process.
- The existing `.expect(...)` strings already *are* the human-readable
  reason; `catch_unwind` hands that message back for the record.
- No `amd_baseline`/`score` signature change ⇒ no churn in `baseline_score`
  or the ssi-scoring / integration tests. A `Result` conversion's only real
  benefit — a precise, typed, *recoverable* error — is unused here.

## Design

Three changes, all in `src/main.rs` (trusted harness code). `ssi-scoring`
signatures and all output **formats** stay frozen (Invariant 1); this is
content and control flow, not contract.

### 1. Guard the trusted baseline + score with `catch_unwind`

A small helper traps a panic and returns the payload message as `Err(String)`:

```rust
/// Run a trusted-path closure, converting a panic into an Err carrying the
/// panic message. Mirrors the containment the contestant path already gets
/// from its subprocess+watchdog, for the in-process baseline/score path.
fn catch<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T, String> {
    std::panic::catch_unwind(f).map_err(panic_message)
}

/// Best-effort extraction of a panic payload's message.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic (non-string payload)".to_string()
    }
}
```

In the loop, the baseline+score is computed inside one guarded closure and
funnels into the **same** `failed = Some(reason); break` path every other gate
uses:

```rust
let base = match catch(|| {
    let base_perm = ssi_scoring::amd_baseline(pat);
    score(pat, &base_perm)
}) {
    Ok(b) => b,
    Err(msg) => {
        failed = Some(format!("{name}: trusted baseline/score panicked — {msg}"));
        break;
    }
};
```

The final `score(pat, &perm1)` on the contestant permutation
(`src/main.rs:172`) is also trusted-path and gets the same guard.

### 2. Record the failure reason in `results.tsv`

Thread the reason into the note column, keeping the frozen 5-column format
(`ts \t status \t score \t fill \t note`). On FAIL the note becomes the reason,
followed by ` | ` + the user note when present; tabs/newlines are already
stripped by `parse_note`, and the reason is sanitized the same way:

```
1782160339	FAIL	NaN	NaN	arrow_big: trusted baseline/score panicked — matrix too large for i32-indexed AMD | note: retry knob
```

OK rows are unchanged. This is column *content*, not a format change.

### 3. Guarantee scratch disposal on every exit

Replace `std::process::exit(1)` with a `main() -> ExitCode` return and wrap the
scratch dir in a `Drop` guard so cleanup runs on **every** return path — OK,
FAIL, and any stray unwind through `main`:

```rust
struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}
```

`main` returns `ExitCode::FAILURE` on FAIL / early-exit and
`ExitCode::SUCCESS` on OK, instead of calling `std::process::exit`. The
existing explicit `remove_dir_all(&scratch)` at loop end is removed (the guard
subsumes it). Note: `catch_unwind` already prevents the trusted-path panic from
reaching `main`'s unwind at all — the guard is the belt-and-suspenders backstop
for anything else, and the mechanism that makes `exit`-free cleanup work.

## Out of scope / unchanged

- Score definition, gates, `order()` signature, output **formats** (Invariant 1).
- `score.json` is still written **only on OK** — a FAIL records solely a
  `results.tsv` row, matching today's gate-failure behavior. No FAIL score.json.
- `amd_baseline` keeps its panic-on-error signature; the harness catches it.
- The grader runs this same harness binary (Yukon `cargo run --release`), so
  fixing `src/main.rs` fixes both local and graded runs — one code path
  (Invariant 2). No separate grader change.

## Testing

The real `i32` overflow is not practically triggerable in a test — it needs a
pattern with >`i32::MAX` (~2.1 B) entries, i.e. 8+ GB of index buffers. So the
seams are tested directly, not by manufacturing an oversized matrix:

- **Unit — `panic_message`:** `&str` payload, `String` payload, and non-string
  payload each map to the expected string.
- **Unit — `catch`:** returns `Ok(v)` for a non-panicking closure; returns
  `Err(msg)` carrying the message for a panicking one (e.g. one that
  `panic!("boom")`s), and the process survives the call. This is the seam that
  proves an `amd_baseline`/`score` panic becomes a recoverable `Err` rather
  than a process abort — independent of what triggers the panic.
- **Unit — `ScratchDir` Drop guard:** create a dir via the guard, drop it,
  assert the dir no longer exists; a second drop / missing dir is a no-op.
- **Unit — FAIL note composition:** the helper that builds the note column
  yields `"<reason> | <user note>"` when a note is present and `"<reason>"`
  when it is absent, with tabs/newlines stripped.
- **Regression:** existing `tests/` (exact_equivalence, scorer_crosscheck,
  time_cap) and the Invariant-4 closed-form scorer tests still pass; a normal
  OK run still writes `score.json` and an `OK` row, and a normal gate FAIL
  (e.g. a nondeterministic/invalid contestant perm) now carries its reason in
  the `results.tsv` note column.

To keep these unit-testable, `catch`, `panic_message`, the note composition,
and the `ScratchDir` guard live in a small `mod`/functions that a `#[cfg(test)]`
block in the binary crate (or a `src/` module with tests) can reach directly —
no need to spawn the whole binary.

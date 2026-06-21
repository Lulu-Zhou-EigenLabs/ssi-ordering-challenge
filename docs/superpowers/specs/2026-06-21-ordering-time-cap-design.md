# Prompt subprocess-enforced ordering time cap — design

**Date:** 2026-06-21
**Status:** approved (brainstorming)
**Scope:** the local public harness only (`ssi-ordering-challenge/`). The private grader is NOT modified.

## Problem

The local harness has a 5 s per-matrix time cap (`TIME_CAP_PER_MATRIX`,
`src/main.rs:54`) but it is **measured, not enforced**: `order()` is called
synchronously on the main thread (`src/main.rs:94`), runs to completion, and
only *afterward* is the elapsed time compared to the cap (`src/main.rs:101-109`).
A runaway `order()` therefore blocks the whole run for its full duration before
being declared too slow.

Empirically (full-corpus run, 2026-06-20), the current submission's `order()`
exceeded the cap badly on **dense** matrices — e.g. n=2343 / nnz=202664 took
**340 s**, n=670 / nnz=333040 took 32 s — while the trusted scoring path
(`load_corpus_jsonl` + `amd_baseline` + `score`) handled the largest matrices
(n≈340k) in ≤0.11 s. The cost is in `order()`, driven by **density (nnz/n)**,
not dimension: the exact-min-fill path is gated only by `n ≤ 3000` and is
O(n²·deg²) with fill-in blowup on dense graphs.

## Goal

Make the harness **kill** an `order()` that exceeds the cap **promptly** (at the
cap, not after it finishes) and report an **explicit, actionable** error, so a
contestant immediately knows their ordering is too slow and on which matrix.

## Why a cap, not a score term (settled)

The score stays a pure, deterministic, hardware-independent function of
`(pattern, permutation)` — that is what makes a local score equal the graded
score (Invariant 2) and satisfies the twice-run determinism gate. Ordering cost
is correctly modeled as a **hard pass/fail constraint**, not a graded term,
because (a) in the target workload (interior-point KKT systems) the ordering is
computed once and reused across many factorizations, so its cost is amortized;
(b) any score weighting would require an arbitrary amortization factor N; and
(c) wall-clock time is hardware-dependent and non-deterministic, so it cannot
enter the score without breaking exact-grader equivalence. Cheap deterministic
op-counters (allocation / API-call counting) are gameable; ungameable ones
(cachegrind, wasm-fuel) are too slow or too invasive. So: **cap only.**

## Approach: subprocess + SIGKILL (mirrors the grader)

`order()` is a synchronous function; it cannot be interrupted from its own
thread. To truly kill it we run it in a **child process** and SIGKILL the child
at the cap — exactly the mechanism the grader already uses
(`grader/runner/src/watchdog.rs`). The harness binary gains two modes:

- **Parent mode** (default, `cargo run`): per matrix, compute the AMD baseline +
  score in-process (trusted, fast), then spawn *itself* as a worker subprocess
  to run `order()` under a watchdog that SIGKILLs at the cap. Run the worker
  **twice** (determinism gate), read back each permutation, validate (bijection),
  score via `ssi_scoring::score`.
- **Worker mode** (`--worker <jsonl_path> <line_index> <out_perm>`): the ONLY
  process that runs contestant code. Load exactly that one pattern via
  `ssi_scoring::load_pattern_jsonl_line(path, line_index)` (already built for
  this purpose), run `ordering::order()`, write the permutation to `<out_perm>`,
  exit 0. A panic / crash exits non-zero with no perm file → parent treats it as
  a FAIL.

### New harness modules (trusted plumbing — NOT scoring, NOT submission code)

- `src/watchdog.rs` — spawn a child command, poll with `try_wait`, SIGKILL on
  time breach; returns a typed outcome (Ok / Timeout / Crashed). Command-agnostic
  so it is unit-testable with `/bin/sleep`. `std`-only (mirrors the grader's
  time-kill path, which needs no `libc` — `Child::kill()` sends SIGKILL).
- `src/perm_io.rs` — binary permutation read/write (little-endian u64 count then
  u64 indices), ported from `grader/runner/src/perm_io.rs`. Trusted harness I/O.

These live under `src/` (the harness), not `src/ordering/` — Invariant 3
(stdlib-only) governs `src/ordering/` only, and the harness may have
dependencies. `std`-only here is a deliberate choice to keep the public repo
lean, not a requirement.

### Index-space consistency

The parent loads the corpus with `load_corpus_jsonl` (file order, blank lines
skipped) while the worker loads one line by raw index with
`load_pattern_jsonl_line`. These can diverge only if the corpus contains blank
lines. To close this (the footgun flagged in the prior review), the parent
sources the worker's `line_index` from the SAME enumeration it used to build its
in-memory corpus, so the index it passes always matches the entry it scores.
(The shipped corpus has no blank lines; this makes correctness independent of
that.) A test pins parent/worker agreement.

## The cap value

Flat **2 s** per `order()` call, replacing the current 5 s constant. 2 s is the
bottom of the cost doc's recommended 2–5 s band (`COMPETITION-VERIFIER-COST §1`).

**Divergence note (documented, not silent):** the grader's watchdog default is
currently 5 s. Local 2 s < grader 5 s means the local gate is *stricter* — the
safe direction ("passes locally ⇒ passes on the server"). This is intentional
for this change; reconciling the grader's default to 2 s is a separate one-line
change in the private repo, out of scope here. The README documents that local
enforces 2 s.

## Error reporting

On a cap breach, FAIL the run (unchanged contract semantics) with an explicit
Stage-B message including the matrix name and its size/density, e.g.:

```
RUN FAILED (Stage B — time cap): matrix 'st_e09' (n=2343, nnz=202664, nnz/n≈87):
  order() exceeded the 2.0s per-matrix cap and was killed.
  Your ordering must return within 2s on every matrix. This matrix is dense
  (high nnz/n), so the cost is in order() itself — gate expensive paths by
  BOTH n and nnz.
```

A worker crash (panic/non-zero exit/missing perm file) FAILs with a distinct
message ("order() crashed / produced no output"), preserving the existing panic
gate's intent.

## Frozen-contract compliance

- `order(pattern: &Pattern) -> Vec<usize>` signature: unchanged.
- Score definition, `score.json` / `results.tsv` formats: unchanged.
- Cap value is an *operating parameter* (cost doc allows 2–5 s), not a contract
  definition — moving 5 s → 2 s is permitted by Invariant 1.
- On a *passing* submission, scores are byte-identical to today: same `order()`,
  same `ssi-scoring` path, merely run across a process boundary.

## Out of scope

- Memory cap (`RLIMIT_AS`): the user asked for time enforcement; the grader has
  its own memory cap via `libc`. Keeping the harness time-only avoids a `libc`
  dependency in the public repo.
- Modifying the grader (its watchdog/worker/perm_io already exist).
- A shared `ssi-harness-runner` crate: considered and rejected for now — the
  local and grader runners each talk only to themselves, and time verdicts are
  hardware-dependent regardless, so shared watchdog code is not load-bearing for
  correctness. Independent `std`-only code keeps the public repo lean. Revisit
  only if mechanism drift becomes a real problem.

## Testing

- **watchdog** (`src/watchdog.rs` unit tests): a command that sleeps past a short
  cap is killed and reported Timeout; a fast command returns Ok; a command that
  exits non-zero is reported Crashed.
- **perm_io** (`src/perm_io.rs` unit tests): round-trip; reject truncated /
  length-mismatched files (ported from the grader's tests).
- **end-to-end regression**: running the harness over the committed sample corpus
  still produces the same valid score as before (the subprocess path yields the
  same permutations and score).
- **cap enforcement**: a test-only slow path (env-gated, e.g. `SSI_TEST_SLEEP_MS`
  honored by the worker) makes the worker exceed the cap; assert the parent
  reports a time-cap FAIL promptly (well under the runaway duration) and exits
  non-zero.
- **index-space agreement**: the pattern the worker loads for `line_index` equals
  corpus entry `line_index` the parent scored.
- existing closed-form / cross-check / exact-equivalence tests stay green.
```

# Simplify `src/ordering/` to call feral-amd

**Date:** 2026-07-03
**Branch:** `simplify-ordering-use-feral-amd`
**Status:** approved (design)

## Problem

`src/ordering/amd.rs` is a 505-line hand-rolled port of quotient-graph AMD
(Amestoy, Davis & Duff), written when the submission directory was
constrained to the Rust standard library. The most recent merged work
(commits `df4e383`…`2b09680`) lifted that constraint: a submission may now
declare permissive, pure-Rust crates in `src/ordering/deps.toml`, and the
purity gate enforces the whole transitive tree is pure Rust and permissively
licensed.

With crates allowed, re-implementing AMD by hand is unnecessary. The trusted
scoring wrapper (`ssi-scoring`) already computes the AMD **baseline** by
calling `feral_amd::amd_order`. The starter submission can call the same crate
directly, deleting the hand-rolled port.

## Consequence (accepted)

Because `ssi-scoring::amd_baseline` and the new `order()` will both call
`feral_amd::amd_order` on the same raw full-symmetric pattern with default
options, the starter submission becomes **byte-identical to the baseline** and
scores **exactly 1.00** on every matrix. This is the intended behavior: the
shipped `src/ordering/` is a minimal, correct, buildable starting point;
contestants improve from there. (Confirmed with the maintainer.)

## Viability (verified)

- `feral-amd 0.2.1` depends **only** on `feral-ordering-core 0.2.1`.
- `feral-ordering-core 0.2.1` has **zero** dependencies.
- Both are MIT, pure Rust, no `build.rs`, no FFI.
- Neither pulls in the C-backed `feral-metis` / `feral-scotch` / `feral-kahip`
  companion crates (verified by reading their `Cargo.toml` manifests).

Therefore the submission purity gate accepts them. These are the exact same
crate names and versions (`=0.2.1`) already pinned by `ssi-scoring`, so they
resolve from the frozen crates.io snapshot with no new lockfile churn beyond
adding the submission-side declaration.

## Changes

### 1. `src/ordering/deps.toml`

Declare the two crates under `[dependencies]`:

```toml
[dependencies]
feral-amd = "0.2.1"
feral-ordering-core = "0.2.1"
```

(`prepare-build.sh` regenerates the harness `[dependencies]` from
`Cargo.toml.in` + this validated `deps.toml`; versions match the `=0.2.1`
pins in `ssi-scoring`.)

### 2. Delete `src/ordering/amd.rs`

The 505-line hand-rolled port is removed entirely.

### 3. `src/ordering/mod.rs`

- Remove `mod amd;`.
- Replace the body of `order()` (currently `amd::order(pattern)`) with the
  CscPattern build + `feral_amd::amd_order` call: convert `pattern.col_ptr` /
  `pattern.row_idx` from `usize` to `i32`, build a
  `feral_ordering_core::CscPattern`, call `feral_amd::amd_order`, map the
  `i32` result back to `Vec<usize>`. This mirrors the body of
  `ssi-scoring`'s `amd_baseline` (`ssi-scoring/src/lib.rs:108`).
- Keep the `order()` signature unchanged (Invariant 1).
- Keep the `SSI_TEST_SLEEP_MS` test hook (used by the harness time-cap test).
- Rewrite the module doc-comment: the "Current approach: AMD … hand-rolled
  quotient graph" narrative becomes "delegates to the `feral-amd` crate,
  declared in `deps.toml`." Keep the contract/purity guidance for contestants.

### 4. Tests in `src/ordering/mod.rs`

The tests are implementation-independent (they assert contract properties, not
the old port's internals), so they carry over:

- **Keep unchanged:** `order_is_a_valid_bijection`, `order_handles_empty`,
  `order_handles_singleton`, `order_handles_no_edges`, `tridiagonal_is_valid`,
  `order_is_deterministic`, `disjoint_cliques_valid`.
- **`arrow_eliminates_hub_last`:** resolve empirically during implementation.
  Run feral-amd on the arrow pattern; keep the strict `perm.last() == 0`
  assertion if it passes. Only relax it (e.g. bijection-only) if feral's AMD
  tie-breaking genuinely places the hub elsewhere. Do not pre-weaken it.

## Invariants

- **Inv 1 (contract frozen):** `order()` signature, score definition, gates,
  output formats — all unchanged. (Absolute score value for the *starter*
  becomes 1.00, which is a value change, not a definition change.)
- **Inv 2 (one scoring path):** untouched. This edits the *submission*, not the
  scorer. `ssi-scoring` remains the sole scoring code path.
- **Inv 3 (submission purity):** honored under the amended policy — the
  declared crates and their whole transitive tree are pure Rust and
  permissively licensed.
- **Inv 4 (closed-form tests):** the scorer's closed-form tests are in
  `ssi-scoring`, untouched. The submission's contract tests are preserved.
- **Inv 5 (green & committed):** `cargo test` must pass at the end; commit at
  the working milestone.

## Verification

1. `bash scripts/prepare-build.sh` (or the documented build-prep step)
   regenerates `Cargo.toml` from the template + new `deps.toml`.
2. `cargo build` — the harness compiles with feral-amd reachable from the
   submission.
3. `cargo test` — all preserved ordering tests pass.
4. Run the harness against the dev corpus; confirm the starter scores 1.00
   (identical to baseline) as expected.
5. Purity/deny gate (`src/purity.rs` / cargo-deny) passes for the new deps.

## Out of scope

- No change to `ssi-scoring`, the grader, `Cargo.toml.in`, or the contract.
- No new ordering algorithm; this is a simplification, not an improvement.

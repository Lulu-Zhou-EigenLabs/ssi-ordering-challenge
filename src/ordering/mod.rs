//! ★ THE SUBMISSION DIRECTORY ★ — the one place a contestant may edit.
//!
//! Implement your fill-reducing ordering here. The contract is frozen:
//!
//! ```ignore
//! pub fn order(pattern: &Pattern) -> Vec<usize>
//! ```
//!
//! You receive ONLY the sparsity pattern (structure, never values, never a
//! right-hand side) and return a permutation: `perm[k]` is the original index
//! eliminated k-th. The permutation must be a bijection of `0..n`, returned
//! deterministically (the harness runs `order` twice and requires identical
//! output) within the per-matrix time cap. Any violation FAILs the whole run.
//!
//! Rules (enforced by the harness's local purity gate, mirroring the grader):
//! - stdlib only — no `[dependencies]`, no build.rs, no FFI/extern, no
//!   proc-macros, no `include!` outside this directory;
//! - you may split this directory into submodules and refactor freely.
//!
//! ## The shipped starter: natural ordering
//!
//! This stub returns the identity permutation (`0, 1, …, n−1`) — the matrix is
//! factored in its given order, with no fill reduction. It is deliberately the
//! WORST sensible baseline so the leaderboard has somewhere to climb from: on
//! the dev corpus it scores well above the AMD baseline (anchored at 1.00).
//! Your job is to beat AMD (score < 1.00).
//!
//! See `memory/` for a worked iteration trajectory and a more advanced
//! nested-dissection + minimum-degree demo (`memory/demo_nd_amd_hybrid.rs.txt`)
//! — note that demo predates the real corpus and uses an exact-MD inner loop
//! that exceeds the time cap on the largest matrices (n ≈ 160k); it is a
//! reference for ideas, not a drop-in.

use crate::pattern::Pattern;

/// Return an elimination ordering for `pattern`.
///
/// Starter implementation: the natural (identity) ordering. Replace this with
/// your fill-reducing algorithm.
pub fn order(pattern: &Pattern) -> Vec<usize> {
    (0..pattern.n).collect()
}

//! # YOUR CODE GOES HERE
//!
//! This is the only place you may modify (split it into submodules under
//! `src/ordering/` and refactor freely).
//!
//! ## The contract
//!
//! Implement [`order`]. It receives the sparsity *pattern* of a symmetric
//! matrix — structure only, no numerical values, no right-hand side — and
//! must return a permutation: `perm[k]` is the original row/column index
//! eliminated k-th.
//!
//! Enforced by the harness:
//! - the result must be a bijection of `0..pattern.n` (validated; a
//!   malformed permutation or a panic fails the run),
//! - the function must be deterministic (the harness runs it twice per
//!   matrix and requires identical output),
//! - it must finish within the per-matrix time cap (10 s).
//!
//! Any valid permutation yields a correct factorization — you cannot break
//! correctness from here. You can only make the factorization cheaper or
//! more expensive. Lower predicted flops = better score.
//!
//! ## Where to start
//!
//! The starter below returns the identity (natural) ordering, which scores
//! ~42x worse than the minimum-degree baseline. Classical directions,
//! roughly in order of effort:
//! - reverse Cuthill-McKee (bandwidth reduction) — easy; scores ~1.64,
//! - your own minimum-degree / minimum-fill variant with better
//!   tie-breaking or approximate degree updates (AMD),
//! - nested dissection via graph partitioning (George 1973; what METIS
//!   does) — the state of the art on grid-like problems,
//! - hybrids: dissection on top, minimum degree on small subgraphs,
//! - local search / refinement on top of any of the above.
//!
//! Record what you tried and why in `src/ordering/memory/`.

use crate::pattern::Pattern;

/// Produce a fill-reducing elimination order for `pattern`.
pub fn order(pattern: &Pattern) -> Vec<usize> {
    // Starter: natural ordering. Replace me.
    (0..pattern.n).collect()
}

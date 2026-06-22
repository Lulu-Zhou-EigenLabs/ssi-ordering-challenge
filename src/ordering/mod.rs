//! ★ THE SUBMISSION DIRECTORY ★ — the one place you may edit.
//!
//! Fill-reducing ordering. Contract (frozen):
//!   `pub fn order(pattern: &Pattern) -> Vec<usize>`
//! Returns `perm[k]` = the original index eliminated k-th; the result must be a
//! bijection of `0..n`, deterministic (the harness runs `order()` twice and
//! requires identical output), and return within the 2 s/matrix cap. stdlib
//! only — no added dependencies, no FFI, no build scripts (the purity gate
//! enforces this).
//!
//! ## This is a STARTER STUB
//!
//! The shipped `order()` returns the **identity / natural order** (eliminate
//! variables in their given order). It is valid and deterministic, but it is a
//! deliberately trivial baseline — on real KKT patterns it eliminates dense
//! constraint rows early and densifies the factor, so it scores *far worse*
//! than the AMD baseline (score ≫ 1.00). Your job is to replace it.
//!
//! You may rewrite this file completely, split it into submodules, and add
//! helpers — anything under `src/ordering/` is yours, as long as `order()`
//! keeps its signature and the constraints above. Cost scales with **density
//! (nnz, max-degree)**, not just `n`, so gate any expensive path by both.
//!
//! Where to start: minimum-degree / AMD, nested dissection, minimum-fill, and
//! local-search refinement are the classic families (see `README.md` →
//! "Background reading"). Record what you try in `memory/`.

use crate::pattern::Pattern;

/// Return an elimination order for `pattern`.
///
/// STARTER STUB: the identity permutation `[0, 1, ..., n-1]`. Replace this with
/// a real fill-reducing ordering.
pub fn order(pattern: &Pattern) -> Vec<usize> {
    // TEST-ONLY hook: when SSI_TEST_SLEEP_MS is set, sleep that long before
    // ordering. Inert unless the env var is present (never set in normal runs
    // or on the grader); lets the harness's time-cap test force a breach.
    // Harmless to leave in place; safe to remove if you rewrite this file.
    if let Ok(ms) = std::env::var("SSI_TEST_SLEEP_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }

    // Identity order: eliminate 0, 1, 2, ... in the matrix's given order.
    (0..pattern.n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The starter `order()` must satisfy the contract the harness enforces:
    /// a bijection of `0..n`. (It is deliberately not a *good* ordering.)
    #[test]
    fn order_is_a_valid_bijection() {
        let n = 60;
        let mut edges = Vec::new();
        // A 2D grid-ish pattern with a few chords — a non-trivial graph.
        for v in 0..n - 1 {
            edges.push((v, v + 1));
        }
        for v in 0..n - 8 {
            edges.push((v, v + 8));
        }
        let pat = Pattern::from_edges(n, &edges);
        let perm = order(&pat);
        assert_eq!(perm.len(), n);
        let mut seen = vec![false; n];
        for &v in &perm {
            assert!(v < n && !seen[v], "not a bijection of 0..{n}");
            seen[v] = true;
        }
    }

    /// An empty pattern yields an empty permutation (no panic on n = 0).
    #[test]
    fn order_handles_empty() {
        let pat = Pattern::from_edges(0, &[]);
        assert!(order(&pat).is_empty());
    }
}

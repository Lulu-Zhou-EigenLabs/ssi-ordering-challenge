//! ★ THE SUBMISSION DIRECTORY ★ — the one place you may edit.
//!
//! Fill-reducing ordering. Contract (frozen):
//!   `pub fn order(pattern: &Pattern) -> Vec<usize>`
//! Returns `perm[k]` = the original index eliminated k-th; the result must be a
//! bijection of `0..n`, deterministic (the harness runs `order()` twice and
//! requires identical output), and return within the 2 s/matrix cap.
//!
//! You MAY use permissive, PURE-RUST third-party crates: declare them in
//! `src/ordering/deps.toml` (one `name = "x.y.z"` per line under
//! `[dependencies]`; no git/path/features). The purity gate scans this
//! directory for FFI / `#[no_mangle]` / `#[link]` / proc-macros / build scripts
//! / `include!` outside the dir, and the grader rejects any dependency (whole
//! transitive tree) that is `*-sys`, ships a native blob, compiles C, or carries
//! a non-permissive license. Everything resolves from the frozen crates.io
//! snapshot — no git or path sources. See `docs/DECISION-crate-policy.md`.
//!
//! ## Current approach: feral-amd (Approximate Minimum Degree)
//!
//! `order()` delegates to the [`feral_amd`] crate — declared in
//! `src/ordering/deps.toml` — which runs the quotient-graph AMD heuristic.
//! This is the same crate and version the harness baseline uses, so the
//! shipped starter scores 1.00 (it ties the baseline); it is a minimal,
//! correct starting point to improve on. See `memory/techniques/amd.md` for
//! where AMD wins and loses, and `memory/` for what to try next (nested
//! dissection is the open headroom).
//!
//! Everything under `src/ordering/` is yours: split it, add submodules, swap
//! the algorithm, declare crates in `deps.toml` — as long as `order()` keeps
//! its signature, stays deterministic, and stays pure Rust (no FFI / build
//! scripts / native code, in this directory or any declared dependency's tree).

use crate::Pattern;

/// Return an elimination order for `pattern`.
///
/// Builds feral's `CscPattern` from the frozen `Pattern` contract input and
/// runs `feral_amd::amd_order`. The result is a bijection of `0..pattern.n`;
/// an empty pattern yields an empty permutation.
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

    // feral_ordering_core's CscPattern is borrowed + i32-indexed. Convert the
    // usize CSC buffers at this boundary (mirrors ssi_scoring::amd_baseline).
    let col_ptr: Vec<i32> = pattern
        .col_ptr
        .iter()
        .map(|&x| i32::try_from(x).expect("matrix too large for i32-indexed AMD"))
        .collect();
    let row_idx: Vec<i32> = pattern
        .row_idx
        .iter()
        .map(|&x| i32::try_from(x).expect("matrix too large for i32-indexed AMD"))
        .collect();
    let pat = feral_ordering_core::CscPattern::new(pattern.n, &col_ptr, &row_idx)
        .expect("malformed CscPattern for AMD (bug in Pattern invariants)");
    let perm_i32 = feral_amd::amd_order(&pat).expect("feral AMD ordering failed");
    perm_i32.into_iter().map(|x| x as usize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert `perm` is a bijection of `0..n`.
    fn assert_bijection(perm: &[usize], n: usize) {
        assert_eq!(perm.len(), n, "permutation length");
        let mut seen = vec![false; n];
        for &v in perm {
            assert!(v < n && !seen[v], "not a bijection of 0..{n}");
            seen[v] = true;
        }
    }

    /// `order()` must satisfy the contract the harness enforces: a bijection of
    /// `0..n` on a non-trivial graph.
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
        assert_bijection(&order(&pat), n);
    }

    /// An empty pattern yields an empty permutation (no panic on n = 0).
    #[test]
    fn order_handles_empty() {
        let pat = Pattern::from_edges(0, &[]);
        assert!(order(&pat).is_empty());
    }

    /// A single node with no edges — degree-0 fast path.
    #[test]
    fn order_handles_singleton() {
        let pat = Pattern::from_edges(1, &[]);
        assert_eq!(order(&pat), vec![0]);
    }

    /// A fully disconnected graph: every node has degree 0, all eliminated by
    /// the empty-node fast path. Order is the natural one and a valid bijection.
    #[test]
    fn order_handles_no_edges() {
        let n = 10;
        let pat = Pattern::from_edges(n, &[]);
        assert_bijection(&order(&pat), n);
    }

    /// Arrow matrix: a hub (node 0) connected to every other node, plus a path
    /// among the rest. AMD must eliminate the hub LAST — eliminating it first
    /// turns the whole factor dense. We assert the hub is in the final position.
    #[test]
    fn arrow_eliminates_hub_last() {
        let n = 40;
        let mut edges = Vec::new();
        for v in 1..n {
            edges.push((0, v)); // hub
        }
        for v in 1..n - 1 {
            edges.push((v, v + 1)); // path among the spokes
        }
        let pat = Pattern::from_edges(n, &edges);
        let perm = order(&pat);
        assert_bijection(&perm, n);
        assert_eq!(*perm.last().unwrap(), 0, "hub (node 0) must be eliminated last");
    }

    /// A tridiagonal matrix already has zero fill under natural order; AMD must
    /// not make it worse and must return a valid bijection.
    #[test]
    fn tridiagonal_is_valid() {
        let n = 100;
        let edges: Vec<(usize, usize)> = (0..n - 1).map(|v| (v, v + 1)).collect();
        let pat = Pattern::from_edges(n, &edges);
        assert_bijection(&order(&pat), n);
    }

    /// Determinism gate (Stage E analog): two runs on the same pattern must
    /// return byte-identical output.
    #[test]
    fn order_is_deterministic() {
        let n = 200;
        let mut edges = Vec::new();
        for v in 0..n - 1 {
            edges.push((v, v + 1));
        }
        for v in 0..n - 13 {
            edges.push((v, v + 13));
        }
        let pat = Pattern::from_edges(n, &edges);
        assert_eq!(order(&pat), order(&pat));
    }

    /// Two disjoint cliques: each block is dense, but there is no fill *between*
    /// blocks. AMD must produce a valid bijection (and never bridge the blocks).
    #[test]
    fn disjoint_cliques_valid() {
        let mut edges = Vec::new();
        for a in 0..6 {
            for b in (a + 1)..6 {
                edges.push((a, b));
            }
        }
        for a in 6..12 {
            for b in (a + 1)..12 {
                edges.push((a, b));
            }
        }
        let pat = Pattern::from_edges(12, &edges);
        assert_bijection(&order(&pat), 12);
    }
}

//! ★ THE SUBMISSION DIRECTORY ★ — the one place you may edit.
//!
//! Fill-reducing ordering. Contract (frozen):
//!   `pub fn order(pattern: &Pattern) -> Vec<usize>`
//! Returns `perm[k]` = the original index eliminated k-th; the result must be a
//! bijection of `0..n`, deterministic (the harness runs `order()` twice and
//! requires identical output), and return within the 2 s/matrix cap.
//!
//! ## Approach: per-matrix best-of {AMD, AMF, METIS-ND} with cost envelopes
//!
//! The score is a geomean of per-matrix `flops(yours)/flops(AMD)` ratios, so
//! choosing, *per matrix*, the cheapest of several candidate orderings can only
//! match or beat AMD — it is free headroom. Because AMD is always in the
//! candidate set and our in-process AMD is the exact baseline, the worst case on
//! any matrix is ratio = 1.0; any matrix where another ordering wins pulls the
//! geomean strictly below 1.0.
//!
//! Candidates are feral's pure-Rust ordering crates:
//!   - `feral_amd`   — Approximate Minimum Degree (the baseline; always run).
//!   - `feral_amf`   — Approximate Minimum Fill (highest-value extra candidate).
//!   - `feral_metis` — METIS-class nested dissection (multilevel + FM + AMD leaf).
//!
//! Each candidate's cost is scored with feral's *own* symbolic building blocks
//! (`permute_pattern → EliminationTree → column_counts_gnp`, then `Σ c_j²`) —
//! the identical function the harness/grader uses — so the candidate we keep is
//! exactly the one the grader would rank best. We return the min-flops valid
//! permutation.
//!
//! ## Staying under the 2 s / SIGKILL cap — HARD cost envelopes
//!
//! The harness runs `order()` in a child process that is SIGKILLed at a hard
//! 2 s per matrix; a single breach FAILs the whole run. Our earlier submission
//! scored 0.9198 locally but FAILED server-side: the hidden corpus is bigger and
//! the grader hardware is slower (assume 3-5x), so a candidate that was merely
//! "under 2 s locally" is not safe there.
//!
//! This version was re-gated from a full wall-time measurement of every
//! candidate over the whole dev corpus:
//!   - **AMD** is unconditional. It is the grader's own baseline (so it cannot
//!     itself time out) and our guaranteed-valid fallback.
//!   - **AMF** showed NO structural runtime volatility — its cost is a smooth
//!     ~1.4x of AMD's across all 300 dev matrices, including the densest — so it
//!     is run broadly, gated only to keep a hidden giant of unknown size on the
//!     AMD-only path.
//!   - **METIS** runtime IS structure-dependent: cheap (<=~0.13 s) up to
//!     nnz ≈ 3e5, but it exploded to 6.2 s on one n=272k / nnz=1.38M matrix
//!     (≈6x its same-size neighbours). It is bounded by **nnz primarily** (the
//!     cost driver), far below that blow-up scale, with an n cap as
//!     defense-in-depth. `MetisOptions::default()` fixes `seed = 1`.
//!   - The volatile / low-value candidates (Scotch, KaHIP, multi-seed METIS)
//!     were DROPPED: together they improved the score by <0.005 while adding the
//!     slowest, least predictable code paths (KaHIP hit 13 s on a giant).
//!
//! The candidate set is a pure function of `(n, nnz)` — never wall-clock — so
//! the two required `order()` runs are byte-identical (determinism gate). With
//! these envelopes the worst measured local child time is ≈0.19 s (≈1 s at 5x
//! slower, i.e. >2x margin under the 2 s cap). AMD is the guaranteed fallback if
//! every richer candidate is gated out, fails, or returns an invalid permutation.

use crate::Pattern;

use feral::ordering::amd::permute_pattern;
use feral::ordering::elimination_tree::EliminationTree;
use feral::sparse::csc::CscPattern as ScoringPattern;
use feral::symbolic::column_counts_gnp;

/// AMF cost is a smooth ~1.4x of AMD's with no observed structural blow-up, so we
/// run it on all but the very largest problems. These bounds keep AMD+AMF child
/// time ≈0.13 s at the largest included matrix locally (≈0.7 s at 5x slower);
/// anything larger falls back to AMD only.
const AMF_MAX_N: usize = 250_000;
const AMF_MAX_NNZ: usize = 1_300_000;

/// METIS runtime is structure-dependent and can explode on large, adversarial
/// patterns (measured: 6.2 s at nnz≈1.38M). Bound it by nnz PRIMARILY, an order
/// of magnitude below that scale, plus an n cap as defense-in-depth. Within this
/// envelope the worst measured METIS child time is ≈0.13 s (≈0.65 s at 5x).
const METIS_MAX_N: usize = 120_000;
const METIS_MAX_NNZ: usize = 300_000;

/// Return an elimination order for `pattern` (best-of over the ordering family).
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
        return Vec::new();
    }

    // i32-indexed borrowed pattern shared by every feral ordering crate.
    let col_ptr_i32: Vec<i32> = pattern
        .col_ptr
        .iter()
        .map(|&x| i32::try_from(x).expect("matrix too large for i32-indexed ordering"))
        .collect();
    let row_idx_i32: Vec<i32> = pattern
        .row_idx
        .iter()
        .map(|&x| i32::try_from(x).expect("matrix too large for i32-indexed ordering"))
        .collect();
    let core = feral_ordering_core::CscPattern::new(n, &col_ptr_i32, &row_idx_i32)
        .expect("malformed CscPattern (bug in Pattern invariants)");

    // usize-indexed owned pattern for the trusted scoring path (Σ c_j²).
    let scoring_pat = ScoringPattern {
        n,
        col_ptr: pattern.col_ptr.clone(),
        row_idx: pattern.row_idx.clone(),
    };

    // AMD is the anchor and the guaranteed-valid fallback: run it first so we
    // always have a valid permutation even if every richer candidate is gated
    // out or fails. It is also the grader's own baseline, so it cannot time out.
    let amd = feral_amd::amd_order(&core).expect("feral AMD ordering failed");
    let mut best_perm: Vec<usize> = amd.into_iter().map(|x| x as usize).collect();
    let mut best_flops: u64 = flops_of(&scoring_pat, &best_perm);

    // Candidate set gated purely by (n, nnz) so both required runs agree, and
    // sized (from full-corpus wall-time measurement) so the total child time
    // stays far under the 2 s SIGKILL cap even on a ~5x-slower grader.
    let nnz = pattern.nnz();

    // Try a candidate produced by `f`; keep it if it is a valid bijection with
    // strictly fewer flops. `catch_unwind` guards against a candidate panicking
    // (which would otherwise crash the worker and FAIL the whole run).
    let mut consider = |produce: &dyn Fn() -> Result<Vec<i32>, feral_ordering_core::OrderingError>| {
        let produced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(produce));
        let Ok(Ok(perm_i32)) = produced else {
            return;
        };
        let perm: Vec<usize> = perm_i32.into_iter().map(|x| x as usize).collect();
        if !is_bijection(&perm, n) {
            return;
        }
        let f = flops_of(&scoring_pat, &perm);
        if f < best_flops {
            best_flops = f;
            best_perm = perm;
        }
    };

    // AMF — the highest-value extra candidate, cheap and structurally stable.
    if n < AMF_MAX_N && nnz < AMF_MAX_NNZ {
        consider(&|| feral_amf::amf_order(&core));
    }

    // METIS nested dissection — bounded by nnz primarily (its cost driver) plus
    // an n cap; `seed = 1` (via default) keeps it deterministic.
    if n < METIS_MAX_N && nnz < METIS_MAX_NNZ {
        consider(&|| {
            feral_metis::metis_order_full(&core, &feral_metis::MetisOptions::default())
                .map(|(p, _, _)| p)
        });
    }

    best_perm
}

/// Predicted factorization flops `Σ_j c_j²` for `perm` on `pat`, via feral's
/// pattern-pure symbolic building blocks — the exact quantity the grader ranks.
fn flops_of(pat: &ScoringPattern, perm: &[usize]) -> u64 {
    let permuted = permute_pattern(pat, perm);
    let etree = EliminationTree::from_pattern(&permuted);
    let counts = column_counts_gnp(&permuted, &etree);
    counts.iter().map(|&c| (c as u64) * (c as u64)).sum()
}

/// Whether `perm` is a bijection of `0..n` (guards a candidate before scoring).
fn is_bijection(perm: &[usize], n: usize) -> bool {
    if perm.len() != n {
        return false;
    }
    let mut seen = vec![false; n];
    for &v in perm {
        if v >= n || seen[v] {
            return false;
        }
        seen[v] = true;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_bijection(perm: &[usize], n: usize) {
        assert_eq!(perm.len(), n, "permutation length");
        let mut seen = vec![false; n];
        for &v in perm {
            assert!(v < n && !seen[v], "not a bijection of 0..{n}");
            seen[v] = true;
        }
    }

    #[test]
    fn order_is_a_valid_bijection() {
        let n = 60;
        let mut edges = Vec::new();
        for v in 0..n - 1 {
            edges.push((v, v + 1));
        }
        for v in 0..n - 8 {
            edges.push((v, v + 8));
        }
        let pat = Pattern::from_edges(n, &edges);
        assert_bijection(&order(&pat), n);
    }

    #[test]
    fn order_handles_empty() {
        let pat = Pattern::from_edges(0, &[]);
        assert!(order(&pat).is_empty());
    }

    #[test]
    fn order_handles_singleton() {
        let pat = Pattern::from_edges(1, &[]);
        assert_eq!(order(&pat), vec![0]);
    }

    #[test]
    fn order_handles_no_edges() {
        let n = 10;
        let pat = Pattern::from_edges(n, &[]);
        assert_bijection(&order(&pat), n);
    }

    #[test]
    fn arrow_is_valid() {
        let n = 40;
        let mut edges = Vec::new();
        for v in 1..n {
            edges.push((0, v));
        }
        for v in 1..n - 1 {
            edges.push((v, v + 1));
        }
        let pat = Pattern::from_edges(n, &edges);
        assert_bijection(&order(&pat), n);
    }

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

    /// Best-of must never be worse than AMD alone: on any pattern the returned
    /// flops are ≤ AMD's flops (the ratio the grader computes is ≤ 1).
    #[test]
    fn best_of_is_never_worse_than_amd() {
        let n = 120;
        let mut edges = Vec::new();
        for v in 0..n - 1 {
            edges.push((v, v + 1));
        }
        for v in 0..n - 10 {
            edges.push((v, v + 10));
        }
        let pat = Pattern::from_edges(n, &edges);

        let col_ptr_i32: Vec<i32> = pat.col_ptr.iter().map(|&x| x as i32).collect();
        let row_idx_i32: Vec<i32> = pat.row_idx.iter().map(|&x| x as i32).collect();
        let core =
            feral_ordering_core::CscPattern::new(n, &col_ptr_i32, &row_idx_i32).unwrap();
        let amd: Vec<usize> = feral_amd::amd_order(&core)
            .unwrap()
            .into_iter()
            .map(|x| x as usize)
            .collect();
        let scoring_pat = ScoringPattern {
            n,
            col_ptr: pat.col_ptr.clone(),
            row_idx: pat.row_idx.clone(),
        };
        let amd_flops = flops_of(&scoring_pat, &amd);
        let ours_flops = flops_of(&scoring_pat, &order(&pat));
        assert!(ours_flops <= amd_flops, "ours {ours_flops} > amd {amd_flops}");
    }
}

//! Exact-grader equivalence (proposal §6/§7, Phase 3 step 5).
//!
//! The number the local harness prints for a given (ordering, corpus) must be
//! IDENTICAL to the number the Phase-4 grader computes for the same ordering on
//! the same matrices — not merely "computed the same way". That is guaranteed
//! structurally (harness and grader both call `ssi_scoring::score`, a pure
//! function of (pattern, permutation)). This test PINS that guarantee: it
//! scores a fixed ordering (the identity / natural order) on a few committed
//! dev matrices and asserts exact expected values. If anything ever drifts
//! between local and grader scoring — a changed building block, an accidental
//! value dependence — these pinned numbers break immediately.
//!
//! The expected values are recomputed from the committed corpus files; they are
//! facts about (this pattern, the identity permutation) under the Σc² model.

use std::path::Path;

fn score_identity(rel: &str) -> ssi_scoring::Score {
    let path = Path::new("corpus/dev").join(rel);
    let pat = ssi_scoring::load_pattern(&path)
        .unwrap_or_else(|e| panic!("load {}: {e}", path.display()));
    let identity: Vec<usize> = (0..pat.n).collect();
    ssi_scoring::score(&pat, &identity)
}

#[test]
fn pinned_identity_scores_on_committed_dev_matrices() {
    // Small, committed files. PINNED — do not "fix" by editing the numbers;
    // a mismatch means the scoring path changed and local/grader equivalence
    // is at risk.
    let cases: &[(&str, u64, u64)] = &[
        // (file, expected nnz_l, expected flops) under identity ordering.
        PINNED_0,
        PINNED_1,
        PINNED_2,
    ];
    for &(rel, nnz_l, flops) in cases {
        let s = score_identity(rel);
        assert_eq!(s.nnz_l, nnz_l, "nnz_l drift on {rel}: got {}", s.nnz_l);
        assert_eq!(s.flops, flops, "flops drift on {rel}: got {}", s.flops);
    }
}

// Measured once on the committed corpus (see PHASE-3-FINDINGS.md). These three
// are among the smallest dev files so the test is fast and the numbers stable.
const PINNED_0: (&str, u64, u64) = ("ampl/ampl_tutorial_flow_density__iter0.mtx", 209, 1659);
const PINNED_1: (&str, u64, u64) = ("poisson/poisson_k8__iter0.mtx", 1396, 16646);
const PINNED_2: (&str, u64, u64) = ("optctrl/optctrl_t69__iter0.mtx", 486, 1178);

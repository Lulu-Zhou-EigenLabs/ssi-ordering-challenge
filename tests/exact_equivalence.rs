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

/// Score the identity ordering on the corpus matrix whose `source` is `src`,
/// loaded from the committed JSONL sample through the shared scoring-wrapper
/// reader (the same path the harness and grader use — Invariant 2).
fn score_identity(src: &str) -> ssi_scoring::Score {
    let path = Path::new("corpus/dev/patterns.jsonl");
    let corpus = ssi_scoring::load_corpus_jsonl(path)
        .unwrap_or_else(|e| panic!("load {}: {e}", path.display()));
    let (_, pat) = corpus
        .iter()
        .find(|(s, _)| s == src)
        .unwrap_or_else(|| panic!("source {src} not in {}", path.display()));
    let identity: Vec<usize> = (0..pat.n).collect();
    ssi_scoring::score(pat, &identity)
}

#[test]
fn pinned_identity_scores_on_committed_dev_matrices() {
    // Small, committed sample matrices. PINNED — do not "fix" by editing the
    // numbers; a mismatch means the scoring path changed and local/grader
    // equivalence is at risk.
    let cases: &[(&str, u64, u64)] = &[
        // (source, expected nnz_l, expected flops) under identity ordering.
        PINNED_0,
        PINNED_1,
        PINNED_2,
    ];
    for &(src, nnz_l, flops) in cases {
        let s = score_identity(src);
        assert_eq!(s.nnz_l, nnz_l, "nnz_l drift on {src}: got {}", s.nnz_l);
        assert_eq!(s.flops, flops, "flops drift on {src}: got {}", s.flops);
    }
}

// Measured once on the committed JSONL sample. These are among the smallest
// sample matrices so the test is fast and the numbers stable. `gilbert` (n=1001)
// is a hub-last arrow/star under natural order: each leaf column 0..999 couples
// only to the hub (node 1000), the hub eliminated last. That is zero fill — leaf
// columns cost c_j = 2, the hub c = 1 — so nnz_l = 2*1000 + 1 = 2001 and
// flops = 4*1000 + 1 = 4001, a closed-form cross-check that the numbers are
// genuine, not transcribed.
const PINNED_0: (&str, u64, u64) = ("st_e09", 8, 18);
const PINNED_1: (&str, u64, u64) = ("ex8_5_2", 36, 148);
const PINNED_2: (&str, u64, u64) = ("gilbert", 2001, 4001);

//! Candidate elimination orderings for best-of-k selection. AMD default is
//! always first (guarantees no regression); AMD variants and RCM are added
//! under a DETERMINISTIC size/density gate (no wall-clock — a wall-clock
//! dependence would let the harness's two runs diverge and FAIL the
//! determinism gate). See memory/techniques/best-of-k.md.

use super::rcm::rcm_order;
use crate::Pattern;
use feral_amd::{amd_order_opts, AmdOptions};
use feral_ordering_core::CscPattern;

/// A labeled candidate permutation (new-to-old bijection of `0..n`).
pub struct Candidate {
    pub label: &'static str,
    pub perm: Vec<usize>,
}

// Deterministic gate thresholds. First guesses; tune from the first full run's
// per-matrix timing. All candidates are near-linear, and AMD default alone is
// known cap-safe on the whole corpus (it is the baseline).
const SMALL_N: usize = 20_000;
const SMALL_NNZ: usize = 500_000;
const MID_N: usize = 100_000;
const MID_NNZ: usize = 2_000_000;

/// Run AMD with the given options against prebuilt i32 CSC buffers.
fn amd_variant(n: usize, col_ptr: &[i32], row_idx: &[i32], opts: &AmdOptions) -> Vec<usize> {
    let pat = CscPattern::new(n, col_ptr, row_idx).expect("malformed CscPattern for AMD");
    let (perm, _) = amd_order_opts(&pat, opts).expect("feral AMD ordering failed");
    perm.into_iter().map(|x| x as usize).collect()
}

/// Build the candidate set for `pattern`. Always includes `amd_default` first.
pub fn candidates(pattern: &Pattern) -> Vec<Candidate> {
    let n = pattern.n;
    let nnz = pattern.nnz();

    // i32 CSC buffers, built once and shared across AMD variants.
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

    let mut out: Vec<Candidate> = Vec::new();

    // 1. AMD default — ALWAYS, first (index 0). Guarantees score ≤ 1.0.
    out.push(Candidate {
        label: "amd_default",
        perm: amd_variant(n, &col_ptr, &row_idx, &AmdOptions::default()),
    });

    // Deterministic tiering by (n, nnz).
    let small = n <= SMALL_N && nnz <= SMALL_NNZ;
    let mid = n <= MID_N && nnz <= MID_NNZ;

    // AMD variant: suppress dense-row deferral except true hubs (dense_alpha<0).
    let opts_no_defer = AmdOptions {
        aggressive: true,
        dense_alpha: -1.0,
    };
    // AMD variant: earlier dense-row deferral.
    let opts_alpha5 = AmdOptions {
        aggressive: true,
        dense_alpha: 5.0,
    };
    // AMD variant: later dense-row deferral.
    let opts_alpha20 = AmdOptions {
        aggressive: true,
        dense_alpha: 20.0,
    };

    if small {
        // Full set: 3 AMD variants + RCM (5 candidates total).
        out.push(Candidate {
            label: "amd_no_defer",
            perm: amd_variant(n, &col_ptr, &row_idx, &opts_no_defer),
        });
        out.push(Candidate {
            label: "amd_alpha5",
            perm: amd_variant(n, &col_ptr, &row_idx, &opts_alpha5),
        });
        out.push(Candidate {
            label: "amd_alpha20",
            perm: amd_variant(n, &col_ptr, &row_idx, &opts_alpha20),
        });
        out.push(Candidate {
            label: "rcm",
            perm: rcm_order(pattern),
        });
    } else if mid {
        // Reduced set: 1 AMD variant + RCM (3 candidates total).
        out.push(Candidate {
            label: "amd_no_defer",
            perm: amd_variant(n, &col_ptr, &row_idx, &opts_no_defer),
        });
        out.push(Candidate {
            label: "rcm",
            perm: rcm_order(pattern),
        });
    } else {
        // Large/dense: default + ONE cheap AMD variant only (2 candidates).
        out.push(Candidate {
            label: "amd_no_defer",
            perm: amd_variant(n, &col_ptr, &row_idx, &opts_no_defer),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_bijection(perm: &[usize], n: usize) {
        assert_eq!(perm.len(), n, "length");
        let mut seen = vec![false; n];
        for &v in perm {
            assert!(v < n && !seen[v], "not a bijection of 0..{n}");
            seen[v] = true;
        }
    }

    #[test]
    fn amd_default_always_present_first() {
        let p = Pattern::from_edges(6, &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]);
        let c = candidates(&p);
        assert!(!c.is_empty());
        assert_eq!(c[0].label, "amd_default");
    }

    #[test]
    fn small_matrix_gets_full_set() {
        // Grid-ish small graph → small tier → 5 candidates.
        let n = 100;
        let mut edges = Vec::new();
        for v in 0..n - 1 {
            edges.push((v, v + 1));
        }
        for v in 0..n - 10 {
            edges.push((v, v + 10));
        }
        let p = Pattern::from_edges(n, &edges);
        let c = candidates(&p);
        assert_eq!(c.len(), 5);
        for cand in &c {
            assert_bijection(&cand.perm, n);
        }
    }

    #[test]
    fn all_candidates_are_bijections() {
        let n = 40;
        let mut edges = Vec::new();
        for v in 1..n {
            edges.push((0, v)); // hub
        }
        for v in 1..n - 1 {
            edges.push((v, v + 1));
        }
        let p = Pattern::from_edges(n, &edges);
        for cand in candidates(&p) {
            assert_bijection(&cand.perm, n);
        }
    }
}

//! Exact flop scoring of a permutation, computed through the SAME feral
//! symbolic functions the grader uses (ssi-scoring::score). The number we
//! rank candidates by is byte-for-byte the graded number — no drift.

use crate::Pattern;
use feral::ordering::elimination_tree::EliminationTree;
use feral::sparse::csc::CscPattern;
use feral::symbolic::column_counts_gnp;

/// Symbolic factorization cost of an ordering: `flops = Σ cⱼ²`, `nnz_l = Σ cⱼ`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cost {
    pub flops: u64,
    pub nnz_l: u64,
}

/// Build feral's owned usize `CscPattern` once, to reuse across candidates.
pub fn to_feral_pattern(p: &Pattern) -> CscPattern {
    CscPattern {
        n: p.n,
        col_ptr: p.col_ptr.clone(),
        row_idx: p.row_idx.clone(),
    }
}

/// Score permutation `perm` against a prebuilt feral pattern `base`.
/// `perm[k]` = original index eliminated k-th (new-to-old), a bijection of 0..n.
pub fn score(base: &CscPattern, perm: &[usize]) -> Cost {
    let permuted = feral::ordering::amd::permute_pattern(base, perm);
    let etree = EliminationTree::from_pattern(&permuted);
    let counts = column_counts_gnp(&permuted, &etree);
    let nnz_l: u64 = counts.iter().map(|&c| c as u64).sum();
    let flops: u64 = counts
        .iter()
        .map(|&c| {
            let c = c as u64;
            c * c
        })
        .sum();
    Cost { flops, nnz_l }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    // Closed-form facts, mirroring ssi-scoring's scorer tests. If these pass,
    // our path equals the grader's on these inputs.

    #[test]
    fn dense_3x3_flops_14() {
        // Full 3×3: column counts 3,2,1 → nnz_l=6, flops=9+4+1=14.
        let p = Pattern::from_edges(3, &[(0, 1), (0, 2), (1, 2)]);
        let base = to_feral_pattern(&p);
        let c = score(&base, &identity(3));
        assert_eq!(c.nnz_l, 6);
        assert_eq!(c.flops, 14);
    }

    #[test]
    fn star_5_hub_first_flops_55() {
        // 5×5 star, hub eliminated first → dense factor: counts 5,4,3,2,1 →
        // nnz_l=15, flops=25+16+9+4+1=55.
        let p = Pattern::from_edges(5, &[(0, 1), (0, 2), (0, 3), (0, 4)]);
        let base = to_feral_pattern(&p);
        let c = score(&base, &identity(5));
        assert_eq!(c.nnz_l, 15);
        assert_eq!(c.flops, 55);
    }

    #[test]
    fn tridiagonal_zero_fill_nnz() {
        // Tridiagonal n under natural order → nnz_l = 2n−1 (zero fill).
        let n = 100;
        let edges: Vec<(usize, usize)> = (0..n - 1).map(|i| (i, i + 1)).collect();
        let p = Pattern::from_edges(n, &edges);
        let base = to_feral_pattern(&p);
        let c = score(&base, &identity(n));
        assert_eq!(c.nnz_l, (2 * n - 1) as u64);
    }
}

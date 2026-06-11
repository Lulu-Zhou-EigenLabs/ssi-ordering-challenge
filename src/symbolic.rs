//! Trusted symbolic analysis — the scorer.
//!
//! HARNESS FILE — do not modify. Given a sparsity pattern and a
//! permutation, this computes the exact nonzero count of the Cholesky/LDLᵀ
//! factor L of the permuted matrix and a deterministic flop count, without
//! performing any numeric factorization. The contestant never reports a
//! number; this module derives the score from the permutation alone.
//!
//! Algorithms: Liu's elimination-tree construction with path compression,
//! and the O(|L|) row-subtree traversal for per-column counts
//! (Gilbert–Ng–Peyton family; see Davis, "Direct Methods for Sparse
//! Linear Systems", SIAM 2006, ch. 4).

use crate::pattern::Pattern;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Symbolic {
    /// nnz(L), including the diagonal.
    pub nnz_l: u64,
    /// Deterministic flop proxy: sum over columns j of c_j², where c_j is
    /// the column count of L including the diagonal. Proportional to the
    /// dense-operation cost of the LDLᵀ factorization under this ordering.
    pub flops: u64,
}

const NONE: usize = usize::MAX;

/// Compute the symbolic factorization cost of P·A·Pᵀ.
///
/// `perm[k]` is the original index of the vertex eliminated k-th.
/// The permutation must already have been validated as a bijection.
pub fn analyze(a: &Pattern, perm: &[usize]) -> Symbolic {
    let n = a.n;
    assert_eq!(perm.len(), n);

    // Inverse permutation: pinv[original] = new position.
    let mut pinv = vec![0usize; n];
    for (k, &v) in perm.iter().enumerate() {
        pinv[v] = k;
    }

    // Materialize the permuted pattern B = P·A·Pᵀ in CSC (unsorted rows
    // are fine for the algorithms below).
    let mut counts = vec![0usize; n + 1];
    for c in 0..n {
        counts[pinv[c] + 1] += a.col_ptr[c + 1] - a.col_ptr[c];
    }
    for j in 0..n {
        counts[j + 1] += counts[j];
    }
    let mut cp = counts; // col_ptr of B
    let mut rows = vec![0usize; a.nnz()];
    {
        let mut head: Vec<usize> = cp[..n].to_vec();
        for c in 0..n {
            let jc = pinv[c];
            for &r in a.col(c) {
                rows[head[jc]] = pinv[r];
                head[jc] += 1;
            }
        }
        // restore cp (it was not mutated — we copied heads). nothing to do.
        let _ = &mut cp;
    }

    // Elimination tree of B (Liu 1986, with ancestor path compression).
    let mut parent = vec![NONE; n];
    let mut ancestor = vec![NONE; n];
    for j in 0..n {
        for idx in cp[j]..cp[j + 1] {
            let mut i = rows[idx];
            if i >= j {
                continue;
            }
            while ancestor[i] != NONE && ancestor[i] != j {
                let next = ancestor[i];
                ancestor[i] = j;
                i = next;
            }
            if ancestor[i] == NONE {
                ancestor[i] = j;
                parent[i] = j;
            }
        }
    }

    // Column counts of L via row-subtree traversal: L(i, k) ≠ 0 exactly
    // when k lies on the etree path from some j (with B(i,j) ≠ 0, j < i)
    // up to i. Total work O(nnz(L)).
    let mut count = vec![1u64; n]; // diagonal of every column
    let mut mark = vec![NONE; n];
    for i in 0..n {
        mark[i] = i;
        for idx in cp[i]..cp[i + 1] {
            // Entries of column i with row < i are, by symmetry, the
            // entries of row i in the strict lower triangle.
            let mut k = rows[idx];
            if k >= i {
                continue;
            }
            while mark[k] != i {
                mark[k] = i;
                count[k] += 1;
                match parent[k] {
                    NONE => break,
                    p => k = p,
                }
            }
        }
    }

    let nnz_l: u64 = count.iter().sum();
    let flops: u64 = count.iter().map(|&c| c * c).sum();
    Symbolic { nnz_l, flops }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{arrow, Pattern};

    fn identity(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    #[test]
    fn dense_3x3() {
        // Full 3×3: column counts 3, 2, 1 → nnz(L)=6, flops=9+4+1=14.
        let p = Pattern::from_edges(3, &[(0, 1), (0, 2), (1, 2)]);
        let s = analyze(&p, &identity(3));
        assert_eq!(s.nnz_l, 6);
        assert_eq!(s.flops, 14);
    }

    #[test]
    fn tridiagonal_has_no_fill() {
        let n = 100;
        let edges: Vec<_> = (0..n - 1).map(|i| (i, i + 1)).collect();
        let p = Pattern::from_edges(n, &edges);
        let s = analyze(&p, &identity(n));
        // Every column has count 2 except the last (count 1).
        assert_eq!(s.nnz_l, (2 * n - 1) as u64);
    }

    #[test]
    fn arrow_ordering_matters() {
        let n = 50;
        let p = arrow(n);
        // Hub eliminated first → factor goes completely dense.
        let hub_first = identity(n);
        let dense = analyze(&p, &hub_first);
        assert_eq!(dense.nnz_l, (n * (n + 1) / 2) as u64);
        // Hub eliminated last → essentially no fill.
        let mut hub_last: Vec<usize> = (1..n).collect();
        hub_last.push(0);
        let sparse = analyze(&p, &hub_last);
        assert!(sparse.nnz_l < (3 * n) as u64);
        assert!(dense.flops > 50 * sparse.flops);
    }

    #[test]
    fn permutation_invariance_of_validity() {
        // Any permutation yields a finite, positive cost.
        let p = arrow(20);
        let mut perm: Vec<usize> = (0..20).rev().collect();
        let s = analyze(&p, &perm);
        assert!(s.nnz_l >= 20);
        perm.reverse();
        let s2 = analyze(&p, &perm);
        assert!(s2.flops >= s.flops); // hub-first is the worst case
    }
}

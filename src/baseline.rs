//! Frozen baseline ordering and output validation.
//!
//! HARNESS FILE — do not modify. The baseline anchors the leaderboard at
//! 1.00. In the production grader the baseline is feral's AMD
//! implementation; this prototype ships a deterministic exact
//! minimum-degree ordering, which is the algorithm AMD approximates.

use crate::pattern::Pattern;
use std::collections::HashSet;

/// Exact minimum-degree ordering on the elimination graph.
/// Deterministic: ties break on the lowest vertex index.
pub fn min_degree(p: &Pattern) -> Vec<usize> {
    let n = p.n;
    let mut adj: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for j in 0..n {
        for &i in p.col(j) {
            adj[j].insert(i);
        }
    }
    let mut eliminated = vec![false; n];
    let mut perm = Vec::with_capacity(n);
    for _ in 0..n {
        // Lowest-index vertex of minimum current degree.
        let mut best = NONE;
        let mut best_deg = usize::MAX;
        for v in 0..n {
            if !eliminated[v] && adj[v].len() < best_deg {
                best_deg = adj[v].len();
                best = v;
            }
        }
        let v = best;
        eliminated[v] = true;
        perm.push(v);
        let neigh: Vec<usize> = adj[v].iter().copied().collect();
        for &u in &neigh {
            adj[u].remove(&v);
        }
        // Eliminating v turns its neighborhood into a clique (the fill).
        for a in 0..neigh.len() {
            for b in (a + 1)..neigh.len() {
                let (x, y) = (neigh[a], neigh[b]);
                adj[x].insert(y);
                adj[y].insert(x);
            }
        }
        adj[v] = HashSet::new();
    }
    perm
}

const NONE: usize = usize::MAX;

/// Stage C of the grader: the returned permutation must be a true
/// bijection of 0..n. Anything else disqualifies the matrix.
pub fn validate_permutation(perm: &[usize], n: usize) -> Result<(), String> {
    if perm.len() != n {
        return Err(format!("permutation has length {}, expected {}", perm.len(), n));
    }
    let mut seen = vec![false; n];
    for &v in perm {
        if v >= n {
            return Err(format!("index {} out of range 0..{}", v, n));
        }
        if seen[v] {
            return Err(format!("index {} appears more than once", v));
        }
        seen[v] = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::arrow;
    use crate::symbolic::analyze;

    #[test]
    fn min_degree_avoids_the_arrow_trap() {
        let p = arrow(200);
        let perm = min_degree(&p);
        validate_permutation(&perm, 200).unwrap();
        let s = analyze(&p, &perm);
        // MD eliminates the degree-≤2 chain vertices first and the hub
        // last; the factor stays sparse.
        assert!(s.nnz_l < 700);
    }

    #[test]
    fn validator_rejects_garbage() {
        assert!(validate_permutation(&[0, 1, 1], 3).is_err());
        assert!(validate_permutation(&[0, 1, 5], 3).is_err());
        assert!(validate_permutation(&[0, 1], 3).is_err());
        assert!(validate_permutation(&[2, 0, 1], 3).is_ok());
    }
}

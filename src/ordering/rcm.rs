//! Reverse Cuthill–McKee ordering — a pure-Rust, near-linear bandwidth-reducing
//! heuristic. One best-of-k candidate; kept only when it beats AMD on a matrix.

use crate::Pattern;
use std::collections::VecDeque;

/// Reverse Cuthill–McKee permutation (new-to-old), a bijection of `0..n`.
/// Deterministic: start nodes and BFS neighbor visits are ordered by
/// `(degree, index)`.
pub fn rcm_order(pattern: &Pattern) -> Vec<usize> {
    let n = pattern.n;
    if n == 0 {
        return Vec::new();
    }
    let degree: Vec<usize> = (0..n).map(|j| pattern.col(j).len()).collect();

    // Deterministic start-node order: ascending (degree, index).
    let mut start_order: Vec<usize> = (0..n).collect();
    start_order.sort_by_key(|&v| (degree[v], v));

    let mut visited = vec![false; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);

    for &start in &start_order {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut queue: VecDeque<usize> = VecDeque::new();
        queue.push_back(start);
        while let Some(v) = queue.pop_front() {
            order.push(v);
            let mut nbrs: Vec<usize> = pattern
                .col(v)
                .iter()
                .copied()
                .filter(|&w| !visited[w])
                .collect();
            nbrs.sort_by_key(|&w| (degree[w], w));
            for w in nbrs {
                if !visited[w] {
                    visited[w] = true;
                    queue.push_back(w);
                }
            }
        }
    }

    order.reverse();
    order
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
    fn empty_is_empty() {
        let p = Pattern::from_edges(0, &[]);
        assert!(rcm_order(&p).is_empty());
    }

    #[test]
    fn path_is_bijection() {
        let n = 50;
        let edges: Vec<(usize, usize)> = (0..n - 1).map(|i| (i, i + 1)).collect();
        let p = Pattern::from_edges(n, &edges);
        assert_bijection(&rcm_order(&p), n);
    }

    #[test]
    fn disconnected_covers_all_components() {
        // Two disjoint paths; every node must appear exactly once.
        let mut edges = Vec::new();
        for i in 0..5 {
            edges.push((i, i + 1));
        }
        for i in 10..15 {
            edges.push((i, i + 1));
        }
        let p = Pattern::from_edges(16, &edges);
        assert_bijection(&rcm_order(&p), 16);
    }

    #[test]
    fn deterministic() {
        let n = 200;
        let mut edges = Vec::new();
        for v in 0..n - 1 {
            edges.push((v, v + 1));
        }
        for v in 0..n - 7 {
            edges.push((v, v + 7));
        }
        let p = Pattern::from_edges(n, &edges);
        assert_eq!(rcm_order(&p), rcm_order(&p));
    }
}

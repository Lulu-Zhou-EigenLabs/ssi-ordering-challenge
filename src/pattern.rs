//! Sparsity patterns and the deterministic development corpus.
//!
//! HARNESS FILE — do not modify. The pattern type is the only input a
//! contestant ordering ever sees: structure, never values, never a
//! right-hand side.

/// Sparsity pattern of a symmetric matrix, stored as the *full* (both
/// triangles) pattern in compressed-sparse-column form, diagonal omitted.
///
/// Invariants (enforced by the constructor):
/// - `col_ptr.len() == n + 1`
/// - row indices within each column are sorted, unique, in `0..n`,
///   and never equal to the column index
/// - the pattern is structurally symmetric: (i,j) present iff (j,i) present
#[derive(Clone)]
pub struct Pattern {
    pub n: usize,
    pub col_ptr: Vec<usize>,
    pub row_idx: Vec<usize>,
}

impl Pattern {
    /// Build a symmetric pattern from an edge list. Each (r, c) pair is
    /// symmetrized, deduplicated, and the diagonal is dropped.
    pub fn from_edges(n: usize, edges: &[(usize, usize)]) -> Pattern {
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &(r, c) in edges {
            assert!(r < n && c < n, "edge out of range");
            if r == c {
                continue;
            }
            adj[c].push(r);
            adj[r].push(c);
        }
        let mut col_ptr = Vec::with_capacity(n + 1);
        let mut row_idx = Vec::new();
        col_ptr.push(0);
        for list in adj.iter_mut() {
            list.sort_unstable();
            list.dedup();
            row_idx.extend_from_slice(list);
            col_ptr.push(row_idx.len());
        }
        Pattern { n, col_ptr, row_idx }
    }

    /// Off-diagonal structural nonzeros (both triangles).
    pub fn nnz(&self) -> usize {
        self.row_idx.len()
    }

    /// Iterate row indices of column `j`.
    pub fn col(&self, j: usize) -> &[usize] {
        &self.row_idx[self.col_ptr[j]..self.col_ptr[j + 1]]
    }
}

/// Deterministic 64-bit LCG so the corpus is identical on every machine.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, m: usize) -> usize {
        (self.next_u64() % m as u64) as usize
    }
}

/// 2D 5-point finite-difference grid (k × k Laplacian pattern).
pub fn grid2d(k: usize) -> Pattern {
    let n = k * k;
    let mut edges = Vec::new();
    for x in 0..k {
        for y in 0..k {
            let v = x * k + y;
            if x + 1 < k {
                edges.push((v, (x + 1) * k + y));
            }
            if y + 1 < k {
                edges.push((v, x * k + y + 1));
            }
        }
    }
    Pattern::from_edges(n, &edges)
}

/// 3D 7-point finite-difference grid (k × k × k).
pub fn grid3d(k: usize) -> Pattern {
    let n = k * k * k;
    let idx = |x: usize, y: usize, z: usize| (x * k + y) * k + z;
    let mut edges = Vec::new();
    for x in 0..k {
        for y in 0..k {
            for z in 0..k {
                let v = idx(x, y, z);
                if x + 1 < k {
                    edges.push((v, idx(x + 1, y, z)));
                }
                if y + 1 < k {
                    edges.push((v, idx(x, y + 1, z)));
                }
                if z + 1 < k {
                    edges.push((v, idx(x, y, z + 1)));
                }
            }
        }
    }
    Pattern::from_edges(n, &edges)
}

/// Saddle-point / KKT pattern:  [ H  Aᵀ ]
///                              [ A  0  ]
/// H is an nh×nh banded pattern with random extra couplings; A is an
/// m×nh constraint Jacobian pattern with a few entries per row. The
/// (2,2) block is structurally zero — the shape that makes these
/// matrices indefinite and ordering-sensitive in interior-point methods.
pub fn kkt(nh: usize, m: usize, seed: u64) -> Pattern {
    let n = nh + m;
    let mut rng = Lcg::new(seed);
    let mut edges = Vec::new();
    let bw = 40; // coupling bandwidth — KKTs from discretized problems are local
    // H block: tridiagonal band + ~2 nearby random couplings per row
    for i in 0..nh {
        if i + 1 < nh {
            edges.push((i, i + 1));
        }
        for _ in 0..2 {
            let off = 1 + rng.below(bw);
            let j = if rng.below(2) == 0 { i.saturating_sub(off) } else { (i + off).min(nh - 1) };
            if j != i {
                edges.push((i, j));
            }
        }
    }
    // A block: each constraint touches 3–6 primal variables in a local window
    for c in 0..m {
        let row = nh + c;
        let center = c * nh / m;
        let deg = 3 + rng.below(4);
        for _ in 0..deg {
            let off = rng.below(2 * bw);
            let j = (center + off).min(nh - 1);
            edges.push((row, j));
        }
    }
    Pattern::from_edges(n, &edges)
}

/// Arrow matrix: one hub vertex adjacent to every other vertex, plus a
/// chain. The canonical example of why ordering matters: eliminating the
/// hub first produces a completely dense factor; eliminating it last
/// produces almost no fill.
pub fn arrow(n: usize) -> Pattern {
    let mut edges = Vec::new();
    for v in 1..n {
        edges.push((0, v));
        if v + 1 < n {
            edges.push((v, v + 1));
        }
    }
    Pattern::from_edges(n, &edges)
}

/// A named development corpus, generated deterministically at startup.
pub fn dev_corpus() -> Vec<(String, Pattern)> {
    vec![
        ("arrow_2000".to_string(), arrow(2000)),
        ("grid2d_30".to_string(), grid2d(30)),
        ("grid2d_60".to_string(), grid2d(60)),
        ("grid2d_90".to_string(), grid2d(90)),
        ("grid3d_10".to_string(), grid3d(10)),
        ("grid3d_14".to_string(), grid3d(14)),
        ("kkt_600_200".to_string(), kkt(600, 200, 42)),
        ("kkt_2000_700".to_string(), kkt(2000, 700, 7)),
        ("kkt_4000_1500".to_string(), kkt(4000, 1500, 99)),
    ]
}

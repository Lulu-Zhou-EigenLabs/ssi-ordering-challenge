# George 1973 — Nested Dissection of a Regular Finite Element Mesh

## Citation
George, J. Alan (1973). "Nested dissection of a regular finite element mesh."
*SIAM Journal on Numerical Analysis*, 10(2): 345–363.
doi:10.1137/0710032, JSTOR 2156361.

## Core Idea

Nested dissection is a divide-and-conquer reordering for sparse symmetric
systems arising from finite-element meshes. The steps:

1. Build the adjacency graph of the matrix (vertices = unknowns, edges =
   nonzero off-diagonal entries).
2. Find a small *separator* S whose removal splits the graph into two
   (roughly equal) subgraphs A and B with no edges between them.
3. Recurse on A and B independently.
4. Number all vertices in A first, then all in B, then the separator S last.
5. Perform Cholesky factorization in this ordering.

Because A and B are disconnected once S is removed, eliminating A's variables
creates no fill-in in B's columns and vice versa. Fill-in is confined to
interactions *within* each subgraph and *within* the separator. Recursing
further limits fill within the subgraphs by the same argument.

## Why It Reduces Fill

At each level of recursion the separator has size O(√n) for a planar/2D-mesh
graph (by the planar separator theorem). The separator's contribution to
fill is at most O(separator_size²) = O(n) per level, and there are O(log n)
levels, giving total fill O(n log n) for 2D problems.

For a k×k grid (n = k²):
- Fill-in: O(n log n) = O(k² log k)
- Factorization ops: O(n^{3/2}) = O(k³)

Compare with natural (row-by-row) ordering on the same grid:
- Fill-in: O(n^{3/2}) = O(k³)
- Factorization ops: O(n²) = O(k⁴)

## Assumptions / Applicability

- Matrix must be symmetric (for Cholesky); positive-definite assumed.
- Effectiveness depends on finding small, balanced separators — excellent for
  2D meshes, good for 3D meshes (separator O(n^{2/3}), fill O(n^{4/3})).
- The original paper targets regular FE meshes; generalised by Lipton, Rose &
  Tarjan (1979) to arbitrary planar graphs via the planar separator theorem.

## Mapping to our order() Contract

- Input: adjacency structure of the matrix (CSC format available).
- Output: a permutation (bijection 0..n).
- Separator finding: use graph bisection (BFS-level or spectral if time
  allows; greedy vertex bisection for speed).
- Recursion depth is O(log n); each level does O(n) work for separator
  finding → total ordering cost O(n log n), well within 10 s for n ≤ 50k.
- Primary targets: the 2D and 3D grid matrices. For 3D grids the theoretical
  fill advantage is n^{4/3} vs n^{5/3} (natural), which should beat AMD on
  structured meshes.

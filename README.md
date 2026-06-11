# The Fill-Reducing Ordering Challenge

> **Goal.** Given only the *sparsity pattern* of a sparse symmetric indefinite
> matrix, produce the elimination order that makes its LDLᵀ factorization
> cheapest, scored by **predicted factorization flops**, recomputed by a
> trusted symbolic analyzer. Lower is better. The baseline (minimum degree)
> is anchored at **1.00**.

A companion challenge to the [feral](https://github.com/jkitchin/feral)
sparse symmetric indefinite solver, in the spirit of
[ecdsa.fail](https://www.ecdsa.fail/).

---

## Why this matters

Sparse symmetric **indefinite** (SSI) linear systems are the computational
bottleneck of interior-point optimization (every Newton step factors a KKT
saddle-point matrix), finite-element structural analysis, and constrained
PDE problems. The dominant cost of a sparse direct solve is decided *before
a single floating-point number is touched*: by the **order in which the
unknowns are eliminated**.

When Gaussian elimination removes a variable, it couples all of that
variable's neighbors — entries that were structurally zero in A become
nonzero in the factor L. This is **fill-in**. A good elimination order can
change the factorization cost by *orders of magnitude*; finding the
fill-minimizing order is **NP-hard** (Yannakakis 1981). Sixty years of
research — minimum degree (Markowitz 1957; Tinney & Walker 1967), approximate
minimum degree / AMD (Amestoy, Davis & Duff 1996), nested dissection
(George 1973; METIS, Karypis & Kumar 1998) — has produced strong heuristics
but no closed solution, and recent work applies reinforcement learning and
graph neural networks to the same problem. Every percent you shave here is a
percent off every interior-point iteration in every solver that adopts your
ordering.

The problem has one property that makes it a *perfect* competition seam:
**correctness is decoupled from the objective**. Any valid permutation
produces a correct, solvable factorization with the same inertia. A
contestant cannot break the solver — they can only make it cheaper or more
expensive. The entire search space is safe to explore.

## A concrete example: the arrow matrix

Take an n×n "arrow" matrix — one hub variable coupled to all n−1 others,
which form a chain:

```
        natural order (hub first)          hub eliminated last
        ┌ x x x x x x ┐                    ┌ x x         . ┐
        │ x x x       │                    │ x x x       . │
        │ x   x x     │   eliminate        │   x x x     . │
        │ x     x x   │   the hub    →     │     x x x   . │
        │ x       x x │   first: every     │       x x x . │
        └ x         x ┘   pair couples     └ . . . . . x x ┘
                          → L is DENSE         → almost no fill
```

Eliminating the hub first makes every remaining pair structurally coupled:
L becomes completely dense, n(n+1)/2 nonzeros. Eliminating the hub *last*
leaves the factor nearly as sparse as A. Measured by this repo's harness on
`arrow_2000` (n = 2,000):

| ordering            | predicted flops | ratio vs baseline |
|---------------------|-----------------|-------------------|
| natural (hub first) | 2,668,667,000   | **148,366×**      |
| minimum degree      | 17,987          | 1.00              |

Same matrix, same factorization algorithm, five-orders-of-magnitude cost
difference — purely from the permutation. Real KKT matrices are subtler
versions of this: the zero (2,2) block of a saddle-point system punishes
naive orders the way the arrow's hub does.

## The benchmark, precisely

You are given a Rust harness that, for each matrix in a deterministic
development corpus (2D/3D grid Laplacians, saddle-point KKT patterns, and
the arrow), does the following:

1. **Runs** the frozen baseline ordering and scores it symbolically.
2. **Calls** your `ordering::order(&Pattern) -> Vec<usize>` — *twice*. Both
   runs must return identical permutations (determinism gate) within the
   10 s per-matrix time cap.
3. **Validates** your permutation as a true bijection of `0..n` — wrong
   length, duplicates, or out-of-range indices fail the run, as does a panic.
4. **Recomputes** the score from your permutation with the trusted symbolic
   analyzer (elimination tree + exact column counts of L, Liu 1986 /
   Gilbert–Ng–Peyton; see Davis 2006, ch. 4). Your code never reports a
   number — it returns a permutation, and the harness derives everything
   else.
5. **Scores** the run as

   **score = geomean over matrices of flops(yours) / flops(baseline)**

   where `flops = Σⱼ cⱼ²` over the column counts cⱼ of L (a deterministic,
   hardware-independent proxy proportional to the LDLᵀ operation count).
   **Lower is better.** Ties break on the geometric mean of the fill ratio
   nnz(L). The score is written to `score.json` and a row is appended to
   `results.tsv`.

### What "valid" means

A run is rejected — not scored worse, *rejected* — if any of the following
fails on any matrix:

- **Bijection.** The permutation must contain each of `0..n` exactly once.
- **Determinism.** Two runs on the same pattern must agree exactly.
- **Time cap.** 10 s per matrix (annealing and learned orderings are
  welcome; runaways are not).
- **No panics.**

There are no loopholes: the input is the pattern only (no values, no
right-hand side, no answer to peek at), and the score is a pure function of
your output, computed by code you cannot touch.

### Reference numbers

Measured by this harness, on the shipped corpus:

| ordering                                  | score (geomean flop ratio) |
|-------------------------------------------|----------------------------|
| natural / identity (the shipped starter)  | 42.48                      |
| reverse Cuthill–McKee                     | 1.64                       |
| **minimum degree (frozen baseline)**      | **1.00**                   |
| demo: MD + degree-bounded min-fill tie-break | **0.9496** (best so far; grid2d_90 at 0.593) |
| nested dissection (METIS-class), expected | ≈ 0.5–0.8 on grids — unclaimed |

The baseline has been beaten once, by ~5% — there is plenty of headroom
(the demo still *loses* on the 3D grids and the largest KKT). On the
production grader the baseline is feral's AMD and reference lines for
METIS-style nested dissection and MUMPS are shown.

## How to play

```sh
cargo run --release -- --note "what I tried"
```

That single command builds, validates, scores, writes `score.json`, and
appends one row to `results.tsv` with timestamp, status, score, fill ratio,
and your note.

`cargo test --release` runs the harness's self-checks (including the arrow
sanity tests) — useful for verifying your toolchain before iterating.

### What you can edit

You may modify **anything inside `src/ordering/`** — split it into
submodules, rewrite primitives, refactor freely.

You may **not** touch the harness:

- `src/main.rs`, `src/symbolic.rs`, `src/baseline.rs`, `src/pattern.rs` —
  these are the contract.
- `Cargo.toml` — **no dependencies**, stdlib only, stable toolchain.
- `results.tsv` directly (the harness appends to it for you).

### Memory notes

As you iterate, add Markdown notes under `src/ordering/memory/` capturing
approaches that worked, dead ends, and the reasoning behind important
choices. `memory/2026-06-10-demo-trajectory.md` documents a four-iteration
example session, including a time-cap failure and how it was fixed.

### Important note on openness

This codebase is open to contributions chasing the best score, so memory and
source files may come from different agents. Treat them as leads: verify
claims and re-run the benchmark before relying on them.

## Background reading

- M. Yannakakis, *Computing the minimum fill-in is NP-complete*, SIAM J.
  Alg. Disc. Meth. 2 (1981). — why there is no closed solution.
- T. A. Davis, *Direct Methods for Sparse Linear Systems*, SIAM, 2006. —
  the textbook for everything in `symbolic.rs` (elimination trees, column
  counts, fill).
- P. Amestoy, T. A. Davis, I. S. Duff, *An approximate minimum degree
  ordering algorithm*, SIAM J. Matrix Anal. Appl. 17 (1996). — the
  production baseline.
- A. George, *Nested dissection of a regular finite element mesh*, SIAM J.
  Numer. Anal. 10 (1973); G. Karypis & V. Kumar, *A fast and high quality
  multilevel scheme for partitioning irregular graphs*, SIAM J. Sci. Comput.
  20 (1998). — the state of the art to beat on grid-like problems.
- J. W. H. Liu, *A compact row storage scheme for Cholesky factors using
  elimination trees*, ACM TOMS 12 (1986). — the etree algorithm the scorer
  uses.
- I. S. Duff & J. K. Reid, *The multifrontal solution of indefinite sparse
  symmetric linear systems*, ACM TOMS 9 (1983). — the factorization whose
  cost you are minimizing.
- A. Wächter & L. T. Biegler, *On the implementation of an interior-point
  filter line-search algorithm*, Math. Program. 106 (2006). — where the KKT
  matrices come from.

## Relationship to the production competition

This repository is the **local prototype** of the grader described in
`COMPETITION-PROPOSAL.md`: same contract, same metric, same anti-cheat
structure. The production version swaps in feral's symbolic analysis and
AMD baseline, scores on a hidden stratified slice of feral's ~183k
IPM-harvested KKT corpus (fresh-regenerated per round), and runs submissions
in a no-network/no-filesystem sandbox behind a purity & license gate. See
`docs/HARNESS-DESIGN.md` for the full architecture and the migration path.

## License

MIT.

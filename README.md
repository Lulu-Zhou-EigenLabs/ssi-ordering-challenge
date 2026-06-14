# The Fill-Reducing Ordering Challenge

> **Goal.** Given only the *sparsity pattern* of a sparse symmetric indefinite
> matrix, produce the elimination order that makes its LDLᵀ factorization
> cheapest, scored by **predicted factorization flops**, recomputed by feral's
> own symbolic analysis. Lower is better. The baseline — feral's Approximate
> Minimum Degree (AMD) ordering — is anchored at **1.00**.

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

## The benchmark, precisely

You are given a Rust harness that, for each matrix in a development corpus
(real KKT / saddle-point patterns harvested from interior-point solves —
`corpus/dev/`, 216 matrices, n from 38 to ~160,000), does the following:

0. **Gates** your submission with a local **purity & license check** (a subset
   of the production grader's Stage A): `src/ordering/` must be stdlib-only,
   with no build scripts, FFI, proc-macros, or extra dependencies, and the
   dependency licenses must be permissive (checked with `cargo-deny`).
1. **Runs** feral's AMD baseline ordering and scores it.
2. **Calls** your `ordering::order(&Pattern) -> Vec<usize>` — *twice*. Both
   runs must return identical permutations (determinism gate) within the
   per-matrix time cap (5 s).
3. **Validates** your permutation as a true bijection of `0..n` — wrong
   length, duplicates, or out-of-range indices fail the run, as does a panic.
4. **Recomputes** the score from your permutation with feral's own symbolic
   analysis (elimination tree + exact Gilbert–Ng–Peyton column counts of L).
   Your code never reports a number — it returns a permutation, and the
   harness derives everything else.
5. **Scores** the run as

   **score = geomean over matrices of flops(yours) / flops(AMD)**

   where `flops = Σⱼ cⱼ²` over the column counts cⱼ of L (a deterministic,
   hardware-independent proxy proportional to the LDLᵀ operation count).
   **Lower is better.** Ties break on the geometric mean of the fill ratio
   nnz(L). The score is written to `score.json` and a row is appended to
   `results.tsv`.

### One scoring path, shared with the grader

Both the local harness and the private grader compute the score by calling the
**same** functions in the `ssi-scoring` crate — feral's pattern-pure symbolic
building blocks (`symmetric_pattern → permute_pattern → EliminationTree →
column_counts_gnp`, then `nnz(L) = Σ cⱼ`, `flops = Σ cⱼ²`). The score is a pure
function of `(pattern, permutation)`, so **the number you see locally is the
number the grader computes** for the same ordering on the same matrices. There
is no separate code path that could drift.

The AMD baseline is pinned to `feral_amd::amd_order` on the raw full-symmetric
pattern with default options — deterministic and pattern-only (it never reads
matrix values), so the anchor reproduces exactly from a pattern file.

### What "valid" means

A run is rejected — not scored worse, *rejected* — if any of the following
fails on any matrix:

- **Purity/license.** `src/ordering/` must be stdlib-only and license-clean.
- **Bijection.** The permutation must contain each of `0..n` exactly once.
- **Determinism.** Two runs on the same pattern must agree exactly.
- **Time cap.** 5 s per matrix (annealing and learned orderings are welcome;
  runaways are not).
- **No panics.**

There are no loopholes: the input is the pattern only (no values, no
right-hand side, no answer to peek at), and the score is a pure function of
your output, computed by code you cannot touch.

### Reference numbers

Measured by this harness, on the shipped 216-matrix dev corpus:

| ordering                                       | score (geomean flop ratio vs AMD) |
|------------------------------------------------|-----------------------------------|
| natural / identity (the shipped starter)       | **15.91**                         |
| **feral AMD (baseline)**                        | **1.00**                          |
| nested dissection (METIS-class), expected       | ≈ 0.5–0.9 on the larger/grid-like patterns — unclaimed |

The starter loses badly (15.91×) because the natural order eliminates dense
KKT constraint rows early and densifies the factor. Beating AMD (score < 1.00)
is the game; nested dissection is the classic lead on the larger patterns.

> Note on the demo in `memory/`: `memory/demo_nd_amd_hybrid.rs.txt` is a
> nested-dissection + minimum-degree ordering that beat the *previous*
> synthetic corpus, but its exact minimum-degree inner loop is O(n²) per pivot
> and **exceeds the 5 s cap on the largest real matrices** (n ≈ 160k). It is a
> source of ideas, not a drop-in — a production ordering needs a
> quotient-graph (AMD-style) inner loop to stay within budget at scale.

## How to play

```sh
cargo run --release -- --note "what I tried"
```

That single command runs the purity/license gate, scores every dev matrix,
writes `score.json`, and appends one row to `results.tsv` with timestamp,
status, score, fill ratio, and your note.

`cargo test --release` runs the harness's self-checks (closed-form scorer
tests, the scorer cross-check against an independent oracle, loader agreement,
exact-equivalence pins) — useful for verifying your toolchain before iterating.

### Building from a fresh clone

The scoring wrapper (`ssi-scoring/`) depends on feral. For local development in
this workspace it uses **path** dependencies. To build a standalone fork that
is not next to a feral checkout, flip the two dependencies in
`ssi-scoring/Cargo.toml` from paths to the published crates:

```toml
# ssi-scoring/Cargo.toml — contestant switch
[dependencies]
feral     = "0.11"
feral-amd = "0.2"
feral-ordering-core = "0.2"
```

Then `cargo build --release` and `cargo run --release` work from a bare clone.
The scoring API and your `order()` contract are unchanged either way.

### What you can edit

You may modify **anything inside `src/ordering/`** — split it into
submodules, rewrite primitives, refactor freely.

You may **not** touch the harness:

- `src/main.rs`, `src/pattern.rs`, `src/purity.rs` — the contract and gates.
- `ssi-scoring/` — the trusted scoring wrapper (also used by the grader).
- `Cargo.toml` / `deny.toml` — dependency and license policy.
- `results.tsv` directly (the harness appends to it for you).

The purity gate enforces these mechanically for `src/ordering/`: it rejects
build scripts, FFI/`extern`, `#[no_mangle]`/`#[link]`, proc-macros, `include!`
outside the directory, and any added crate dependency.

### Memory notes

As you iterate, add Markdown notes under `src/ordering/memory/` capturing
approaches that worked, dead ends, and the reasoning behind important
choices.

### Important note on openness

This codebase is open to contributions chasing the best score, so memory and
source files may come from different agents. Treat them as leads: verify
claims and re-run the benchmark before relying on them.

## Background reading

- M. Yannakakis, *Computing the minimum fill-in is NP-complete*, SIAM J.
  Alg. Disc. Meth. 2 (1981). — why there is no closed solution.
- T. A. Davis, *Direct Methods for Sparse Linear Systems*, SIAM, 2006. —
  the textbook for elimination trees, column counts, and fill.
- P. Amestoy, T. A. Davis, I. S. Duff, *An approximate minimum degree
  ordering algorithm*, SIAM J. Matrix Anal. Appl. 17 (1996). — the
  production baseline (feral's AMD).
- A. George, *Nested dissection of a regular finite element mesh*, SIAM J.
  Numer. Anal. 10 (1973); G. Karypis & V. Kumar, *A fast and high quality
  multilevel scheme for partitioning irregular graphs*, SIAM J. Sci. Comput.
  20 (1998). — the state of the art to beat on grid-like problems.
- J. R. Gilbert, E. G. Ng, B. W. Peyton, *An efficient algorithm to compute
  row and column counts for sparse Cholesky factorization*, SIAM J. Matrix
  Anal. Appl. 15 (1994). — the column-count algorithm feral's scorer uses.
- A. Wächter & L. T. Biegler, *On the implementation of an interior-point
  filter line-search algorithm*, Math. Program. 106 (2006). — where the KKT
  matrices come from.

## Relationship to the production competition

This repository is the **contestant-facing template** of the competition
described in `COMPETITION-PROPOSAL.md`: same contract, same metric, same
anti-cheat structure, same feral-backed scoring. The private grader scores
submissions on a hidden, stratified, per-round-regenerated evaluation corpus
(disjoint from this dev slice, drawn from the same distribution) in a
no-network/no-filesystem sandbox behind the same purity & license gate, and
calls the same `ssi-scoring` functions — so a contestant's local score predicts
the graded score exactly. See `docs/HARNESS-DESIGN.md` for the architecture.

## License

MIT.

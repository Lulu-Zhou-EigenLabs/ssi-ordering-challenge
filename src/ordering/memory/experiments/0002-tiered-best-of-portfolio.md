# 0002 — Tiered best-of portfolio (AMD/AMF/METIS/Scotch/KaHIP + parameter diversity)

- **Date:** 2026-07-14
- **Score:** before 0.9246 (shipped best-of AMD/AMF/METIS) → after **0.9124** (weighted bucket geomean flop ratio vs AMD)
- **Tiebreak (fill):** 0.9795 → 0.9749
- **Status:** win

## Hypothesis
The shipped best-of {AMD, AMF, METIS-default} leaves headroom in two places:
1. Cheap near-linear AMD/AMF **dense-threshold variants** (`dense_alpha = -1.0`,
   i.e. defer only true hubs) can win on KKT patterns where the default
   `10·sqrt(n)` threshold defers rows it shouldn't (or vice versa), at ~1x
   AMD cost — so they can run at large scale.
2. On small matrices (lt_1k carries 0.30 of the score; every candidate is
   ~1 ms there) broad **seed/mode diversity** across METIS/KaHIP/Scotch is
   nearly free and each per-matrix win pulls the bucket geomean down.

## What changed (`mod.rs` only)
Candidate tiers, gated purely by `(n, nnz)` (determinism preserved):
- **Unconditional:** AMD default (baseline anchor + guaranteed fallback).
- **n<250k, nnz<1.3M:** AMF default.
- **n<150k, nnz<250k:** AMD(dense_alpha=-1), AMF(dense_alpha=-1).
- **n<120k, nnz<200k:** METIS default (seed 1).
- **n<15k, nnz<80k:** METIS seed 2; METIS seed 3 + nd_to_amd_switch=64; Scotch default.
- **n<10k, nnz<60k:** KaHIP Fast seed 1; METIS seed 4 niparts=12 imbalance=0.10.
- **n<1.2k, nnz<20k:** METIS seeds {5..8,10..13}; METIS seed 9 (switch=48, floor=60);
  KaHIP Fast seeds {2..5}; KaHIP Eco; KaHIP Strong; Scotch seeds {1,7,42};
  AMD(aggressive=false); AMD(dense_alpha=4); AMF(dense_alpha=4).
- AMD's own flops are now scored **lazily** (only when a competing candidate
  exists), so the giant-matrix AMD-only path skips a symbolic pass.

## Result (dev corpus, 300 matrices)
- lt_1k 0.9656 → **0.9449**; 1k_10k 0.9373 → **0.9364**; gt_10k 0.8843 → **0.8701**.
- Weighted score **0.9124**, fill 0.9749. All valid/deterministic, no FAILs.
- Wall time: worst `order()` ≈ **0.25 s** local (was 0.43 s mid-iteration before
  tightening; ≈1.25 s at a 5x-slower grader → still under the 2 s SIGKILL cap).

## Negative results (recorded so they are not retried)
- **Cutting AMF's envelope to nnz<700k** (for speed margin) cost gt_10k
  0.8701→0.8753 (score 0.9124→0.9145). AMF wins on the biggest matrices;
  keep its 1.3M envelope — it is smooth-cost and safe.
- **Micro tier on dense-small patterns** (nnz≥20k at n<1.2k, e.g. n=341/nnz=44k)
  cost ~0.3 s through the full candidate set for negligible score change
  (0.9122 vs 0.9124). Gated out by `nnz < 20_000` for grader safety margin.
- Earlier measured (bench.rs): Scotch produced a **9519x** flop blow-up on the
  n=272878 pathological matrix and KaHIP took 25.9 s there — both must stay
  far from the large tail. METIS took 8.5 s on the same matrix. The nnz≈200k
  METIS envelope is an order of magnitude below that scale.

## Why it won
The score is a geomean of per-matrix min-ratios with AMD always in the set, so
each added (safe) candidate is monotone non-worsening; diversity converts
directly into geomean reduction. The dense-alpha variants are the highest
value-per-ms addition (near-linear cost, occasionally large wins on KKT hub
patterns); partitioner seed diversity mops up small-bucket ties.

## Follow-ups
- The 1k_10k bucket barely moved (0.9373→0.9364): the mid-size envelope is
  time-constrained. A **quotient-graph ND hybrid** tuned for this range is the
  open structural lead (see [nested-dissection](../techniques/nested-dissection.md)).
- Consider constrained/CAMD-style ordering exploiting the KKT 2x2 block
  structure (order primal variables before their constraint rows).
- `bench.rs` (test-only, `#[ignore]`) holds the measurement harness:
  `bench_order_walltime` (slowest order() calls) and `bench_candidates`
  (per-candidate ms + flop ratio on the large tail).

## Links
- Techniques: [amd](../techniques/amd.md), [nested-dissection](../techniques/nested-dissection.md)
- Prior: [0001-amd-quotient-graph](0001-amd-quotient-graph.md)

# Index

The map of the knowledge base. One line per page, grouped by type. Read this
first; keep it current whenever you add, rename, or retire a page.

## Current best
- Best score so far: **0.9186** (weighted-bucket geomean flop ratio vs AMD; fill 0.9804), dev corpus 300 matrices.
- Current `src/ordering/` approach: **per-matrix best-of portfolio** — AMD (anchor) + AMF + METIS + SCOTCH (default + 2 seeds) + KaHIP, each gated by tiered `(n, nnz)` cost envelopes; return the min-predicted-flops valid permutation.
- Buckets: lt_1k 0.9595 / 1k_10k 0.9297 / gt_10k 0.8797.
- See: latest entry in [log.md](log.md) and [experiments/0002](experiments/0002-scotch-kahip-portfolio.md).
- ⚠ The pre-2026-07-12 "0.9992 / AMD-only" claim was stale: the shipped code was already a best-of {AMD,AMF,METIS} scoring 0.9246.

## Literature
_(papers — one note each; see [literature/_TEMPLATE.md](literature/_TEMPLATE.md))_
- _none yet — start from the references in the repo README._

## Techniques
_(algorithm families & primitives — see [techniques/_TEMPLATE.md](techniques/_TEMPLATE.md))_
- [amd.md](techniques/amd.md) — the baseline (anchor = 1.00); strong on dense KKT.
- [nested-dissection.md](techniques/nested-dissection.md) — the lead on grid-like/structured families; the open headroom.

## Experiments
_(hypotheses run against the corpus — see [experiments/_TEMPLATE.md](experiments/_TEMPLATE.md))_
- [0000-identity-baseline.md](experiments/0000-identity-baseline.md) — the starter stub; reference point, not competitive.
- [0001-amd-quotient-graph.md](experiments/0001-amd-quotient-graph.md) — AMD port; matches the baseline (the AMD-vs-AMD ceiling).
- [0002-scotch-kahip-portfolio.md](experiments/0002-scotch-kahip-portfolio.md) — widened best-of portfolio (SCOTCH+KaHIP+seeds); 0.9246→0.9186, current best.

## Open questions
- [open-questions.md](open-questions.md) — the research queue.

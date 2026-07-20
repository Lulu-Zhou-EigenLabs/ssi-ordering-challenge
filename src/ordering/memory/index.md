# Index

The map of the knowledge base. One line per page, grouped by type. Read this
first; keep it current whenever you add, rename, or retire a page.

## Current best
- Best score so far: **0.9124** (weighted bucket geomean flop ratio vs AMD; fill 0.9749), dev corpus 300 matrices.
- Current `src/ordering/` approach: **tiered best-of portfolio** — AMD (always) + AMF + dense-alpha variants + METIS/Scotch/KaHIP seed/mode diversity, tier envelopes gated by (n, nnz); each candidate scored with feral's own symbolic flops, min kept.
- Worst local `order()` wall time ≈0.25 s (≈1.25 s at a 5x-slower grader; 2 s cap).
- See: latest entry in [log.md](log.md).

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
- [0001-amd-quotient-graph.md](experiments/0001-amd-quotient-graph.md) — AMD port; 0.9992, matches the baseline (the AMD-vs-AMD ceiling).
- [0002-tiered-best-of-portfolio.md](experiments/0002-tiered-best-of-portfolio.md) — tiered best-of over feral ordering crates + parameter diversity; 0.9246 → 0.9124.

## Open questions
- [open-questions.md](open-questions.md) — the research queue.

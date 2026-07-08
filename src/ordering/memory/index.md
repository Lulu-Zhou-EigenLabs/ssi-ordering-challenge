# Index

The map of the knowledge base. One line per page, grouped by type. Read this
first; keep it current whenever you add, rename, or retire a page.

## Current best
- Best score so far: **0.9740** (weighted geomean flop ratio vs AMD; fill 0.9912), dev corpus 300 matrices (buckets lt_1k 0.9706 / 1k_10k 0.9865 / gt_10k 0.9671).
- Current `src/ordering/` approach: **best-of-k** — AMD default + AMD variants + RCM, scored by feral's exact path, argmin returned (`mod.rs` + `candidates.rs` + `scoring.rs` + `rcm.rs`).
- See: latest entry in [log.md](log.md) and [experiments/0002-best-of-k.md](experiments/0002-best-of-k.md).

## Literature
_(papers — one note each; see [literature/_TEMPLATE.md](literature/_TEMPLATE.md))_
- _none yet — start from the references in the repo README._

## Techniques
_(algorithm families & primitives — see [techniques/_TEMPLATE.md](techniques/_TEMPLATE.md))_
- [amd.md](techniques/amd.md) — the baseline (anchor = 1.00); strong on dense KKT.
- [nested-dissection.md](techniques/nested-dissection.md) — the lead on grid-like/structured families; the open headroom.
- [best-of-k.md](techniques/best-of-k.md) — meta-framework: score k candidates with feral's exact path, return the cheapest; AMD always a candidate ⇒ score ≤ 1.0. **Implemented 2026-07-08, score 0.9740.**

## Experiments
_(hypotheses run against the corpus — see [experiments/_TEMPLATE.md](experiments/_TEMPLATE.md))_
- [0000-identity-baseline.md](experiments/0000-identity-baseline.md) — the starter stub; reference point, not competitive.
- [0001-amd-quotient-graph.md](experiments/0001-amd-quotient-graph.md) — AMD port; 0.9992, matches the baseline (the AMD-vs-AMD ceiling).
- [0002-best-of-k.md](experiments/0002-best-of-k.md) — best-of-k (AMD variants + RCM, exact self-scoring); 1.0000 → 0.9740, first sub-1.0 win.

## Open questions
- [open-questions.md](open-questions.md) — the research queue.

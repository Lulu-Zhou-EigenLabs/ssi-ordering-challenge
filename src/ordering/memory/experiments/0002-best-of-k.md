# 0002 — Best-of-k over AMD variants + RCM with exact self-scoring

- **Date:** 2026-07-08
- **Score:** before 1.0000 → after **0.9740** (weighted geomean flop ratio vs AMD; lower is better). Fill tiebreak 0.9912.
- **Status:** win

## Hypothesis
The benchmark score is a pure function of `(pattern, perm)` computed by public
`feral` symbolic functions, so we can compute the exact grader flop proxy
`Σcⱼ²` ourselves and pick the cheapest of several candidate orderings. With AMD
default always in the set, we can never regress below 1.0, and any matrix where
a variant or RCM beats AMD pulls the score under 1.0. Tests the
[best-of-k](../techniques/best-of-k.md) framework hosting [AMD](../techniques/amd.md)
variants + RCM.

## What changed
New submodules under `src/ordering/`:
- `scoring.rs` — `(flops, nnz_l)` via feral's exact path (`permute_pattern →
  EliminationTree → column_counts_gnp`), same code the grader calls.
- `rcm.rs` — pure-Rust reverse Cuthill–McKee, deterministic `(degree, index)`
  ordering.
- `candidates.rs` — AMD default (always) + AMD variants (`dense_alpha ∈ {-1, 5,
  20}`) + RCM, admitted by a **deterministic** `(n, nnz)` tier gate
  (small→5 cands, mid→3, large/dense→2). No wall-clock gate (would break the
  determinism gate).
- `mod.rs` — `order()` now scores every candidate and returns the argmin by
  `(flops, nnz_l)`, strict `<` so AMD default wins ties.
- `deps.toml` — added `feral = "0.11.0"`.

## Result
Per size bucket (geomean flop ratio vs AMD, lower better):

| bucket | weight | flop geomean | fill geomean |
|--------|--------|--------------|--------------|
| lt_1k  | 0.30   | 0.9706       | 0.9905       |
| 1k_10k | 0.30   | 0.9865       | 0.9963       |
| gt_10k | 0.40   | 0.9671       | 0.9880       |
| **weighted** | | **0.9740** | 0.9912 |

- Improvement in **all three** buckets; strongest in the high-weight `gt_10k`
  (large) bucket (0.9671) and `lt_1k` (0.9706). The `1k_10k` bucket moved least.
- Most matrices still tie AMD at ratio 1.000 (AMD is already strong on KKT);
  wins are concentrated on a subset. Example strong win: `st_qpc-m3a`
  (n=30) → 0.676.
- Two full runs gave byte-identical score 0.973986 (corpus-level determinism
  holds). No time-cap pressure at the current gate thresholds; no FAIL.

## Why it won / lost
- **Won** because best-of-k is a strict floor at AMD plus free upside: wherever a
  cheaper AMD variant (different dense-row deferral) or RCM produces less fill on
  a given matrix, we take it, and the exact self-score guarantees the pick equals
  what the grader will credit.
- **Where the room is left:** the `1k_10k` bucket barely moved, and most matrices
  tie AMD — the cheap candidates rarely beat a well-tuned AMD. The remaining
  headroom is a genuinely different *structural* ordering (nested dissection),
  which slots in as one more candidate. See follow-ups.

## Follow-ups
- Per-family attribution (NLP/QCP/QP/QCQP) not yet done — the harness table keys
  by matrix name, not family. Add a family mapping to see which families the
  wins/ties fall in. (→ open-questions.md)
- Add **nested dissection** as a candidate — the main structural headroom,
  especially for the large/structured `gt_10k` bucket. (→
  [nested-dissection.md](../techniques/nested-dissection.md))
- Consider more AMD variants (`aggressive=false`, other `dense_alpha`) once
  per-family data shows where they'd help; measure cost first.

## Links
- Techniques: [best-of-k.md](../techniques/best-of-k.md), [amd.md](../techniques/amd.md), [nested-dissection.md](../techniques/nested-dissection.md)
- Plan: [../plans/2026-07-08-best-of-k.md](../plans/2026-07-08-best-of-k.md)

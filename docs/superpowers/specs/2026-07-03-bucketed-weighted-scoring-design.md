# Bucketed, weighted scoring — design

**Date:** 2026-07-03
**Branch:** `feature/bucketed-weighted-scoring`
**Status:** approved, ready for planning

## Problem

The dev/eval corpus is dominated by small matrices. Under the current metric —
a single geometric mean over every matrix of `flops(yours)/flops(AMD)` — those
small matrices dominate the score by sheer count. But real-world value and
algorithmic difficulty both concentrate in the *large* matrices. A contestant
can win by tuning small matrices and ignoring the large tail, which is backwards
from what the competition should reward.

## Change (score definition)

Bucket each matrix by `n` (matrix dimension, already available as `pat.n` in the
scoring loop), take the geometric mean of the ratio *within* each bucket, then
combine the per-bucket geomeans as a weighted mean.

```
buckets (by n):   lt_1k  = n < 1000
                  1k_10k = 1000 <= n < 10000      (half-open)
                  gt_10k = n >= 10000

weights:          lt_1k = 0.30,  1k_10k = 0.30,  gt_10k = 0.40

score = sum_over_populated( w_b * geomean_b ) / sum_over_populated( w_b )
```

- Boundary values land unambiguously: `999 -> lt_1k`, `1000 -> 1k_10k`,
  `9999 -> 1k_10k`, `10000 -> gt_10k`.
- **Empty buckets renormalize.** A bucket with no matrices contributes nothing to
  the numerator and its weight is removed from the denominator. On the 13-matrix
  dev corpus (all `lt_1k`), the denominator is just `0.30`, so the reported
  score equals the `lt_1k` geomean — a meaningful local number that still
  predicts the graded number for the same set of matrices.
- **Tiebreak uses the identical scheme.** The fill-ratio tiebreak becomes the
  weighted mean of the per-bucket fill-ratio geomeans, consistent with the main
  score.

The **per-matrix ratio computation is unchanged**: `flops(yours)/flops(AMD)` and
`nnz_l(yours)/nnz_l(AMD)`, both still computed by `ssi_scoring::score` (the one
scoring path). Only the *aggregation* over matrices changes.

## Invariant note (deliberate contract revision)

CLAUDE.md Invariant 1 freezes the score DEFINITION. This change is an
owner-authorized, deliberate revision of that definition, not an accidental
drift. It is handled as a revision: re-baseline, and update every place the
score is documented (main.rs docstring, README, RULES.md, results.tsv header,
benchmark.json description). The `order()` signature, the validity gates, and the
`score.json` / `results.tsv` output *formats* are unchanged.

Invariant 2 (one scoring code path) is preserved for free: there is no separate
`grader/` binary — Yukon dispatches this same harness binary via GitHub Actions
(`benchmark.yml`), so the aggregation logic exists in exactly one place.

Invariant 4 (closed-form scorer tests) is untouched: those tests assert
per-matrix scoring math in `ssi-scoring`, which does not change. Only `main.rs`
aggregation changes.

## Implementation location

All aggregation lives in `src/main.rs`. The current loop accumulates two scalars
(`log_ratio_sum`, `log_fill_sum`); replace them with a per-bucket accumulator:

```
struct BucketAcc { log_ratio_sum: f64, log_fill_sum: f64, count: usize }
```

three of them (one per bucket). Inside the existing per-matrix loop, classify by
`pat.n` and add into the matching accumulator. After the loop, combine.

Two small pure, unit-testable helpers:

- `fn size_bucket(n: usize) -> Bucket` — classify (enum or index 0/1/2).
- `fn combine(buckets, weights) -> f64` — renormalized weighted mean of the
  per-bucket geomeans; returns the weighted mean of `exp(log_sum/count)` over
  populated buckets, weights rescaled to sum to 1. Used for BOTH the flop score
  and the fill tiebreak.

The FAIL path is unchanged (still writes `NaN` score + a FAIL row).

## Output formats

### score.json — extend `metrics` only (top-level `score` shape unchanged)

Verified against Yukon's parser (`yukon/src/benchmark/score.ts`): the schema is
`{ score: <finite number> }` with `.passthrough()`. The only hard requirement is
a top-level finite `score`. Extra top-level fields pass through untouched;
`metrics`, if present and JSON-serializable, is captured whole and surfaced in
the PR score report (`scoreReportComment(..., result.scoreMetrics, ...)`). So the
richer `metrics` below is safe and even user-visible in the report.

```json
{
  "score": 0.873421,
  "metrics": {
    "geomean_flop_ratio": 0.873421,
    "geomean_fill_ratio": 0.910200,
    "matrices": 500,
    "weights": { "lt_1k": 0.30, "1k_10k": 0.30, "gt_10k": 0.40 },
    "buckets": {
      "lt_1k":  { "count": 240, "geomean_flop_ratio": 0.910000, "geomean_fill_ratio": 0.950000 },
      "1k_10k": { "count": 180, "geomean_flop_ratio": 0.880000, "geomean_fill_ratio": 0.920000 },
      "gt_10k": { "count": 80,  "geomean_flop_ratio": 0.720000, "geomean_fill_ratio": 0.790000 }
    }
  }
}
```

- Top-level `score` = the new weighted number; this is what Yukon ranks on.
- `metrics.geomean_flop_ratio` = same value as `score` (now the weighted number,
  NOT a global geomean — documented as such).
- `metrics.geomean_fill_ratio` = the weighted fill tiebreak.
- **All three buckets are always listed.** An empty bucket is
  `{ "count": 0, "geomean_flop_ratio": null, "geomean_fill_ratio": null }` so the
  report always shows the full three-row structure and renormalization is visible.
- Bucket keys are `lt_1k` / `1k_10k` / `gt_10k`.

### results.tsv — byte-identical layout

Columns stay `timestamp / status / score / fill_ratio / note`. Only the *values*
of `score` and `fill_ratio` change meaning (now the weighted numbers). The header
comment is updated to describe the bucketed metric; the starter row note is
reconciled. No column added, no parser breakage.

### Terminal table

The per-matrix table is unchanged. After it, print the per-bucket breakdown
(bucket name, count, flop geomean, fill geomean) and the final weighted score +
tiebreak, so a local run shows how the buckets combined.

## Tests

New unit tests in `src/main.rs` (aggregation is here now):

- `size_bucket` boundaries: `999 -> lt_1k`, `1000 -> 1k_10k`, `9999 -> 1k_10k`,
  `10000 -> gt_10k`, plus `0` and a large value.
- `combine`:
  - all three buckets populated -> weighted mean matches hand-computed value
    (e.g. `0.30*0.8 + 0.30*0.9 + 0.40*0.7` from the user's worked example).
  - one bucket empty -> weights renormalize over the two populated.
  - two buckets empty (dev-corpus case) -> score == the single populated
    bucket's geomean (renormalizer collapses to that bucket's weight).

Existing tests unchanged: `ssi-scoring` closed-form tests (Invariant 4),
`scorer_crosscheck`, `corpus` loader tests, `time_cap`.

## Docs to update

- `src/main.rs` module docstring — replace the `score = geomean over corpus`
  block with the bucketed+weighted definition.
- `README.md` and `RULES.md` — score description.
- `results.tsv` header comment + starter row note.
- `benchmark.json` `description` field (mentions "geomean predicted flops").
- A `docs/PHASE-N-FINDINGS.md`-style note is NOT required; this design doc plus
  the plan record the change.

## Out of scope (YAGNI)

- No configurable weights or bucket bounds — the 0.30/0.30/0.40 split and
  1k/10k cuts are fixed constants, matching the request.
- No change to `ssi-scoring` (per-matrix scoring is correct and frozen).
- No new results.tsv columns; per-bucket detail lives in score.json.

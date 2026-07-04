# Bucketed weighted scoring + full-corpus distribution via Git LFS — design

**Date:** 2026-07-03
**Branch:** `feature/bucketed-weighted-scoring`
**Status:** approved, ready for planning

This design covers two related changes shipped together:

1. **Bucketed, weighted scoring** — aggregate the per-matrix ratio by matrix-size
   bucket, then weighted-mean the buckets (sections below).
2. **Full-corpus distribution via Git LFS** — ship the real 300-pattern dev corpus
   in-repo through Git LFS, replacing the 13-pattern smoke-test sample as the
   default (see "Full-corpus distribution via Git LFS").

They belong in one change because bucketed scoring is only meaningful on a corpus
whose buckets are populated: the 13-pattern sample is entirely `lt_1k`, so the two
larger weights would never be exercised locally. The 300-pattern full corpus
populates all three buckets (`lt_1k=147, 1k_10k=108, gt_10k=45`).

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

For the LFS pointer-file detection (corpus distribution):

- A unit test feeds a Git LFS pointer file's content (starts with
  `version https://git-lfs.github.com/spec/v1`) to the detection helper and
  asserts it is recognized as a pointer (so the harness emits the
  `git lfs pull` guidance rather than an opaque JSON parse error).

Existing tests unchanged: `ssi-scoring` closed-form tests (Invariant 4),
`scorer_crosscheck`, `corpus` loader tests, `time_cap`.

## Full-corpus distribution via Git LFS

### Current state (the gap this fixes)

The repo ships ONLY a 13-pattern, ~20 KB smoke-test sample at
`corpus/dev/patterns.jsonl`. The README and `corpus/dev/README.md` tell
contestants to download the "full development corpus (~279 patterns)" from a
**GitHub release asset** (`.../releases/latest/download/patterns.jsonl`) — but
**no such release exists** (`gh release list` is empty), so those `curl`
instructions currently 404. The full corpus was generated but never published.

The real corpus exists on disk in this workspace:
`corpus-generation/corpus/dev/patterns.jsonl` — **300 patterns, ~99 MB**, n up
to ~340k, all three size buckets populated (`lt_1k=147, 1k_10k=108, gt_10k=45`).
This is the canonical full dev corpus for this change.

### Decision

Distribute the full corpus **in-repo via Git LFS**, and make it the **default**
corpus at `corpus/dev/patterns.jsonl` (replacing the 13-pattern sample). Chosen
over the release-asset mechanism for zero-friction UX: after `git clone` the
corpus is simply present, with no manual download / checksum / `SSI_CORPUS_FILE`
step.

Cost trade-off, recorded with the numbers we verified against GitHub's docs
(https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-storage-and-bandwidth-usage):
Git LFS free tier is **10 GiB storage + 10 GiB/month bandwidth** (Free/Pro);
data packs are gone, replaced by **metered billing** beyond the free tier. At
~99 MB/pull that is ~100 clones/month before overage — acceptable for a
competition repo. (Release-asset CDN bandwidth may be cheaper, but the doc only
confirms metered LFS billing; the UX win is judged worth the metered risk.)

### How Git LFS works here

- Add a `.gitattributes` entry:
  `corpus/dev/patterns.jsonl filter=lfs diff=lfs merge=lfs -text`.
- Committing the 99 MB file stores only a ~130-byte **pointer** (oid + size) in
  the git tree; the bytes go to GitHub's LFS store via `git lfs push`.
- On `git clone` with `git-lfs` installed, git transparently smudges the pointer
  back to the real file — the contestant sees the full JSONL, no extra step.
- Per-round rotation = commit a new version of the tracked file.

### Impact surface

- **Default run.** `cargo run --release` now grades all 300 patterns instead of
  13. Slower per run (seconds–minutes vs. instant) but realistic, and it is what
  makes the bucketed score meaningful locally. This is the intended default.
- **CI self-grade (`benchmark.yml`).** `actions/checkout@v4` defaults to
  `lfs: false` and would check out the POINTER, not the bytes. Two cases:
  - When the eval bucket is configured, grading uses the hidden eval corpus via
    `SSI_CORPUS_FILE` and never reads the committed dev corpus — LFS is
    irrelevant to real grading. (No anti-cheat change: the dev corpus is public
    by design; eval stays in its private bucket.)
  - In the milestone-1 self-grade (no eval bucket → grades the committed dev
    corpus), the checkout step must set `lfs: true`, else the harness would try
    to parse a pointer file. The plan adds `lfs: true` to the checkout step.
- **Missing git-lfs (contestant footgun).** A contestant without `git-lfs`
  installed gets the pointer text instead of JSONL, and the harness's JSONL
  reader fails with a parse error. Mitigations:
  - The corpus loader / harness detects a Git LFS pointer file (it begins with
    `version https://git-lfs.github.com/spec/`) and prints a clear message:
    "corpus is a Git LFS pointer — run `git lfs install && git lfs pull`",
    instead of an opaque JSON parse error.
  - README documents the `git-lfs` prerequisite up front.

### Docs impact

The now-obsolete "download the full corpus from a GitHub release" sections in
`README.md` and `corpus/dev/README.md` are REPLACED with the LFS story: the full
corpus ships in-repo via LFS, install `git-lfs` before cloning (or
`git lfs pull` after), and the default run grades the full set. The broken
`releases/latest/download` links are removed. `SSI_CORPUS_FILE` remains
documented as the override seam (e.g. to grade a different corpus or the tiny
sample), and the eval corpus is still never published.

### Not in scope for corpus distribution

- No automation of per-round corpus rotation — a maintainer commits the new file.
- The 13-pattern sample need not be retained at a second path; if a fast
  smoke-test set is wanted later it can be added, but this change simply promotes
  the full corpus to the default. (Flag during planning if you want it kept.)

## Docs to update

- `src/main.rs` module docstring — replace the `score = geomean over corpus`
  block with the bucketed+weighted definition.
- `README.md` and `RULES.md` — score description; and the corpus section
  (release-asset download → Git LFS in-repo).
- `corpus/dev/README.md` — sample-vs-full-download story → LFS story.
- `results.tsv` header comment + starter row note.
- `benchmark.json` `description` field (mentions "geomean predicted flops").
- `.github/workflows/benchmark.yml` — `lfs: true` on the checkout step (for the
  no-eval-bucket self-grade path).
- A `docs/PHASE-N-FINDINGS.md`-style note is NOT required; this design doc plus
  the plan record the change.

## Out of scope (YAGNI)

- No configurable weights or bucket bounds — the 0.30/0.30/0.40 split and
  1k/10k cuts are fixed constants, matching the request.
- No change to `ssi-scoring` (per-matrix scoring is correct and frozen).
- No new results.tsv columns; per-bucket detail lives in score.json.
- No per-round corpus-rotation automation; no GitHub release asset (LFS replaces
  the release-asset mechanism the old docs described).

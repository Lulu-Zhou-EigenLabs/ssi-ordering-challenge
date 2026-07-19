# 0002 — Widen the best-of portfolio: SCOTCH + KaHIP + extra seeds

- **Date:** 2026-07-12
- **Score:** before **0.9246** → after **0.9186** (weighted-bucket geomean flop ratio vs AMD; lower is better)
- **Tiebreak (fill):** 0.9804
- **Status:** win, and safely under the 2 s cap.

## Starting state (correcting the record)
The old memory said best = 0.9992 / AMD-only. That was **stale**: the shipped
`order()` was already a per-matrix best-of {AMD, AMF, METIS} with `(n,nnz)` cost
envelopes, scoring **0.9246** on the 300-matrix dev corpus. All numbers here are
against that real baseline.

## Hypothesis
Best-of over candidate orderings is *monotonic*: adding any extra candidate,
gated so it never breaches the 2 s/matrix cap, can only lower (never raise) the
geomean, because AMD is always in the set and the worst per-matrix ratio stays
1.0. So add more diverse nested-dissection candidates where they are cheap.

## What changed
- Added candidates: `feral_scotch` (default seed), 2 extra SCOTCH seeds on
  small/medium matrices, and `feral_kahip` on small matrices.
- **Dropped** the idea of extra METIS seeds — measured near-zero marginal value.
- Re-derived every gate from a full-corpus `SSI_TIMING` wall-time sweep
  (temporary eprintln instrumentation, since removed). Gates are pure functions
  of `(n, nnz)` so both required `order()` runs pick the identical candidate set
  (determinism gate). Tiers:
  - AMF: `n<250k, nnz<1.3M` (≈7 ms; sole-best on ~60% of the corpus).
  - METIS default: `n<120k, nnz<300k` (≤~140 ms).
  - SCOTCH default: `n<120k, nnz<200k` (capped tighter than METIS; SCOTCH is
    slower, ≤~175 ms at nnz≈280k, so metis+scotch together stay ≈150 ms).
  - SCOTCH extra seeds [1,2]: `n<60k, nnz<130k` (each ≤~55 ms here).
  - KaHIP default: `n<20k, nnz<20k` (slowest/least predictable — 13 s on a
    giant, ≥120 ms already at nnz≈98k; its wins are all on tiny matrices).

## Result
- 300/300 dev matrices valid, deterministic, OK.
- Buckets: lt_1k 0.9656→0.9595, 1k_10k 0.9373→0.9297, gt_10k 0.8843→0.8797.
- **Timing:** worst per-`order()`-call total ≈266 ms locally
  (n=17809, nnz=120632). At an assumed 5× slower grader ≈1.3 s < 2 s cap.

## Why it won
Marginal-value analysis over the timing/flops log:
- **AMF** is the dominant single candidate (sole-best on 177/296 matrices).
- **SCOTCH** finds separators METIS misses — strictly beats the METIS-family
  best on ~10 matrices, with big gains (48%, 36%, 31%, 22%, 12% fewer flops on
  specific patterns); several of those wins come from the *non-default* SCOTCH
  seeds, which is why the extra-seed sweep pays off.
- **KaHIP** adds a handful of small wins on tiny matrices.
- **Extra METIS seeds**: near-zero unique wins — dropped (they were the cause of
  a 1891 ms blow-up in the first, unsafe draft of this experiment).

## Dead ends / cautions
- First draft ran 6 METIS seeds + 3 SCOTCH seeds up to `n<40k, nnz<150k`:
  scored 0.9166 but one matrix hit **1891 ms locally** → would FAIL on the
  slower grader. Lesson: gate multi-candidate sweeps by where each *single* ND
  call is a few ms, not merely "under 2 s locally". Two runs × slower hardware
  eat the margin fast.

## Follow-ups
- Bucket gt_10k (weight 0.40) is the biggest lever and still 0.88 — larger
  structured matrices are gated onto AMD/AMF/METIS only. A near-linear
  quotient-graph ND that can run at n≈100k–340k safely is the next headroom.
- Could tune SCOTCH `ScotchOptions` (n_sep_trials, fm_pass_cap) for quality on
  the medium bucket where time is cheap.

## Links
- Techniques: [amd](../techniques/amd.md), [nested-dissection](../techniques/nested-dissection.md)
- Prior: [0001-amd-quotient-graph](0001-amd-quotient-graph.md)

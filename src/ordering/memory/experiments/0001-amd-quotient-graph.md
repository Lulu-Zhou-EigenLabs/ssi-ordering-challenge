# 0001 — AMD (quotient-graph approximate minimum degree)

- **Date:** 2026-06-30
- **Score:** before ≫1.00 (identity stub) → after **0.9992** (geomean flop ratio vs AMD; lower is better)
- **Tiebreak (fill):** 0.9998
- **Status:** win (matches the baseline; the expected ceiling for an AMD-vs-AMD port)

## Hypothesis
Replace the identity stub with a real AMD ordering. Since the harness baseline
*is* AMD (`feral_amd::amd_order`), a faithful quotient-graph port should land at
≈1.00 — confirming the implementation is correct and giving every later
experiment (nested dissection, min-fill, refinement) a competitive starting
point to beat instead of the non-competitive identity stub.

## What changed
- New `src/ordering/amd.rs`: stdlib-only port of CSparse `cs_amd` (Amestoy,
  Davis & Duff 1996) — quotient graph (variables + elements), approximate
  external degree, mass elimination, supernode/indistinguishable-variable
  detection, aggressive element absorption, dense-node sink, assembly-tree
  postorder via iterative `tdfs`.
- `src/ordering/mod.rs`: `order()` now delegates to `amd::order`; kept the
  `SSI_TEST_SLEEP_MS` time-cap hook. Added structural unit tests (bijection,
  arrow→hub-last, tridiagonal, disjoint cliques, determinism, empty/singleton).

## Result
- 279/279 dev matrices: valid, deterministic, all under the 2 s cap.
- Geomean flop ratio **0.9992**, fill ratio **0.9998** — statistically a tie
  with feral's AMD, as expected.
- Edges the baseline slightly on a handful (e.g. `nuclearvc` 0.993,
  `gams05` 0.995), loses by a hair on none materially. Differences are
  tie-breaking / dense-threshold details between the two AMD variants, not a
  structural advantage either way.

## Why it won / lost
It "won" only in the sense of replacing a far-worse-than-1.00 stub with a
correct AMD. There is **no headroom against AMD by doing AMD** — the small
per-matrix deltas are noise from minor heuristic differences (dense threshold,
hash bucketing, tie ordering). Real gains require a *different* algorithm family
on the structured/grid-like matrices where greedy local MD is beaten globally.

## Follow-ups
- Nested dissection on the large grid-like families is the open headroom — see
  [nested-dissection](../techniques/nested-dissection.md). Logged in open-questions.
- Consider AMD as the small-block ordering *inside* nested dissection (hybrid).

## Links
- Techniques: [amd](../techniques/amd.md), [nested-dissection](../techniques/nested-dissection.md)
- Literature: _(Amestoy-Davis-Duff 1996 note still to be written)_
- Prior: [0000-identity-baseline](0000-identity-baseline.md)

# 2026-06-10 — demo trajectory (starter 42.48 → 0.9496)

Worked example of the iteration loop, with real harness output. Verify
before relying on any claim here (re-run the benchmark).

## iter 0 — starter (natural ordering): score 42.48
- arrow_2000 ratio 148,366x: the hub row is eliminated first and densifies
  everything. Lesson: dense rows/columns must go *last*.
- KKT ratios up to ~645x: the zero (2,2) block means constraint rows act
  like many small hubs.

## iter 1 — reverse Cuthill–McKee: score 1.64
- BFS levelization from a min-degree start vertex, neighbors sorted by
  degree, order reversed. ~30 lines.
- Grids improved most; KKTs and grids still well above 1. Small bandwidth
  is not small fill — RCM optimizes the wrong objective; it just stops
  being catastrophic.

## iter 2 — exact min-degree + min-fill tie-break: RUN FAILED
- `grid3d_14: ordering took 26.05s, cap is 10s`.
- Computing exact fill-in for every min-degree candidate is O(d^2) per
  candidate; 3D-grid elimination degrees grow, so the scan explodes.
- Where it ran, it beat the baseline (grid2d_90 at 0.566). Right idea,
  wrong budget.

## iter 3 — tie-break only while min degree <= 12: score 0.9496
- One-line guard; falls back to plain lowest-index min-degree once the
  graph densifies.
- Per-matrix: grid2d_90 **0.593**, grid2d_60 0.932, kkt_600_200 0.930,
  kkt_2000_700 0.969, arrow 1.000 — but grid3d_14 **1.198** and
  kkt_4000_1500 **1.056** still lose to the baseline.
- First overall win vs the frozen baseline (~5%).

## open leads (untried)
- Nested dissection: theory says ~0.5–0.8 on the grid families
  (George 1973). Biggest expected win, especially the 3D grids where the
  greedy tie-break currently loses; a BFS pseudo-peripheral bisector would
  do for a first cut.
- Quotient-graph MD (element absorption) to kill the O(n)-scan and clique
  costs, then afford a richer tie-break inside the 10 s cap.
- KKT-aware preprocessing: place constraint rows relative to the primal
  block explicitly (cf. feral's LdltCompress for arrow-KKT shapes) —
  kkt_4000_1500 regressed, suggesting the greedy choice mishandles the
  constraint block at scale.
- Local search on the best order found (window reversals, pairwise swaps,
  incremental rescoring): most matrices finish in well under 1 s, so ~9 s
  of the cap is unused.

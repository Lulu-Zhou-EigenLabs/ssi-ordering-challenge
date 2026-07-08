# Log

Chronological record, one line per session. **Append-only, newest at the
bottom.** Never rewrite past entries — if a claim turns out wrong, add a new
line correcting it.

Format:

```
YYYY-MM-DD | score before→after | what you tried | outcome (+ link to experiment page)
```

Scores are geomean flop ratio vs AMD (lower is better; AMD = 1.00). They are
only comparable within the same corpus round — the corpus rebaselines per
round, so note the round if you know it.

---

<!-- newest entries below this line -->
2026-06-30 | ≫1.00 → 0.9992 | ported quotient-graph AMD (cs_amd style) into src/ordering/, replacing the identity stub | win — matches feral's AMD baseline as expected; no headroom by doing AMD-vs-AMD, next gain is nested dissection (see [0001](experiments/0001-amd-quotient-graph.md))
2026-07-08 | 1.0000 → 0.9740 | best-of-k: score k candidates (AMD default + AMD variants dense_alpha∈{-1,5,20} + RCM) with feral's exact flop path, return argmin; deterministic (n,nnz) gate | win — improved all 3 buckets (lt_1k 0.9706 / 1k_10k 0.9865 / gt_10k 0.9671), no regression by construction, no cap pressure. Next: add nested dissection as a candidate (see [0002](experiments/0002-best-of-k.md))

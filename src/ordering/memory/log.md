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
2026-07-14 | 0.9246 → 0.9124 | tiered best-of portfolio: +AMD/AMF dense_alpha=-1 variants (n<150k,nnz<250k), +METIS seeds/Scotch (n<15k), +KaHIP (n<10k), +broad seed/mode diversity in lt_1k; lazy AMD scoring; envelopes re-measured (worst order() 0.25 s local) | win — all three buckets improved; negative results (AMF envelope cut, dense micro tier) recorded (see [0002](experiments/0002-tiered-best-of-portfolio.md))

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
2026-07-12 | 0.9246 → 0.9186 | widened the per-matrix best-of portfolio: added SCOTCH (default + 2 extra seeds) and KaHIP alongside AMD/AMF/METIS, each gated by tiered (n,nnz) envelopes calibrated from a full-corpus SSI_TIMING sweep | win — SCOTCH multi-seed lands big flop wins METIS misses (up to 48% on one matrix); dropped worthless extra METIS seeds; worst order() ≈266 ms/call locally (~1.3 s at 5× grader, safe). NOTE: on-disk best was already 0.9246 (best-of AMD/AMF/METIS), not the 0.9992 the old notes claimed — memory was stale. See [0002](experiments/0002-scotch-kahip-portfolio.md)

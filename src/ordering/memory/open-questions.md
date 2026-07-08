# Open questions

The research queue: leads worth chasing, gaps in the knowledge base, and
hypotheses not yet tested. Add a line whenever you notice one; resolve it by
linking to the page (experiment, technique, or literature note) that answers
it, rather than deleting it — a resolved question is a useful signpost.

## Active
- [ ] Add **nested dissection** as a best-of-k candidate — the main structural
      headroom after [0002](experiments/0002-best-of-k.md), especially the
      high-weight gt_10k bucket. Slots into the existing framework with no
      regression risk (worse candidates are never selected).
- [ ] Per-family attribution (NLP/QCP/QP/QCQP): the harness table keys by matrix
      name, not family. Add a family mapping to see which families the best-of-k
      wins fall in, so candidate tuning is targeted. (raised by [0002](experiments/0002-best-of-k.md))
- [ ] Port the demo ND+AMD hybrid's exact-MD inner loop (O(deg²)/pivot) to a
      quotient-graph minimum degree so it stays under the 2 s cap on dense KKT
      matrices. See [techniques/nested-dissection.md](techniques/nested-dissection.md).
- [ ] AMD is already strong on these patterns — where is the real headroom?
      Hypothesis: nested dissection on the larger / more structured families.
      Confirm per family (NLP/QCP/QP/QCQP) and size bucket.
- [ ] What density (nnz / max-degree) threshold should gate an expensive path
      so it never breaches the cap? Measure, don't guess.
- [ ] Do any ML/RL-guided ordering ideas fit a stdlib-only, deterministic,
      2 s/matrix `order()`? Survey the literature before assuming yes/no.

## Resolved
- [x] Implement the best-of-k framework (AMD default + variants + RCM, exact
      feral self-scoring, deterministic argmin, gated) → done, score 0.9740. See
      [experiments/0002-best-of-k.md](experiments/0002-best-of-k.md) /
      [techniques/best-of-k.md](techniques/best-of-k.md).
- [x] Wall-clock vs deterministic gating: a wall-clock candidate-inclusion guard
      would break the determinism gate; best-of-k uses a **deterministic (n,nnz)**
      gate instead. Settled in [techniques/best-of-k.md](techniques/best-of-k.md).

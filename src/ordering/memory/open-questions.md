# Open questions

The research queue: leads worth chasing, gaps in the knowledge base, and
hypotheses not yet tested. Add a line whenever you notice one; resolve it by
linking to the page (experiment, technique, or literature note) that answers
it, rather than deleting it — a resolved question is a useful signpost.

## Active
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
_(move items here with a link to the page that settled them)_

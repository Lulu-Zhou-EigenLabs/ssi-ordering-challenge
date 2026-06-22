# Nested dissection (ND)

## What it is
A divide-and-conquer ordering: find a small vertex separator that splits the
graph into two balanced halves, order the separator **last**, and recurse on
each half. Numbering the separator last confines fill to the separator block,
which is asymptotically optimal on grid-like / well-structured problems
(George 1973; METIS-class multilevel partitioners, Karypis & Kumar 1998).

## How it works (enough to implement)
- Partition: find a balanced edge/vertex separator (multilevel: coarsen →
  partition coarse graph → refine, e.g. Kernighan–Lin / Fiduccia–Mattheyses).
- Order each part by recursion; place separator vertices after both parts.
- At small subgraphs, switch to a local heuristic (minimum degree) — this is
  the standard **ND + MD hybrid**.

## Cost profile vs the cap
**This is the live risk.** The demo ND+AMD hybrid in this knowledge base wins on
grid-like structure but **breaches the 2 s cap on dense matrices**: its exact-MD
inner loop is O(deg²) per pivot, which explodes on dense KKT rows / hub nodes
even at modest n (and the corpus reaches n ≈ 340k). Any ND path must:
- switch its base-case ordering to a **quotient-graph MD** (near-linear, like
  [AMD](amd.md)), not exact MD; and
- gate the expensive partitioning by **density (nnz / max-degree)**, not just n.

## Where it wins / loses
- **Wins:** larger, structured / grid-like families — the main open headroom,
  since AMD already handles the dense KKTs well.
- **Loses / dangerous:** dense, hub-heavy patterns — separators are large and
  the inner loop blows the cap. Detect and fall back to AMD-style here.

## Status in `src/ordering/`
Lead only — a reference ND+AMD demo exists in memory but is not a drop-in (it
breaks the cap). Porting it to a quotient-graph base case is the top open
question. See [open-questions.md](../open-questions.md).

## Links
- Literature: _(add George 1973 and Karypis–Kumar 1998 notes)_
- Compare: [amd.md](amd.md)

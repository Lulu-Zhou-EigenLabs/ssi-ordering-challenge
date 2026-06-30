# AMD (Approximate Minimum Degree)

## What it is
The competition's baseline ordering, anchored at score **1.00**. A greedy
fill-reducing heuristic: repeatedly eliminate the node of (approximately)
minimum degree, using a quotient-graph representation so degree updates stay
near-linear instead of rescanning the whole graph.

## How it works (enough to implement)
- Represent the eliminated graph as a quotient graph (variables + "elements"),
  so cliques formed by elimination are stored compactly rather than explicitly.
- Each step: pick the min approximate-degree variable, eliminate it, merge its
  neighborhood into a new element, update approximate degrees of affected
  variables. The "approximate" degree bound is what makes it cheap and is the
  key trick vs exact minimum degree.
- Full derivation: see `literature/` once an AMD paper note is written
  (Amestoy, Davis & Duff 1996 — in the repo README references).

## Cost profile vs the cap
Near-linear in practice — the quotient graph keeps per-pivot work bounded. This
is why AMD survives the dense KKT rows that break an exact-MD inner loop. Treat
AMD's cost profile as the bar any expensive candidate path must not exceed.

## Where it wins / loses
- **Wins:** dense KKT / hub-node patterns — already strong here, so headroom
  against AMD on these is thin.
- **Loses (relatively):** large grid-like / structured problems, where global
  nested dissection beats a purely greedy local heuristic.

## Status in `src/ordering/`
**Implemented** — `src/ordering/amd.rs` is a stdlib-only quotient-graph AMD port
(cs_amd style), wired in via `mod.rs::order()`. It is also the harness baseline
(`feral_amd::amd_order`, a different AMD variant) that every run is scored
against, so our score sits at the AMD-vs-AMD ceiling: **0.9992** on the dev
corpus (see [0001](../experiments/0001-amd-quotient-graph.md)). There is no
headroom against AMD by doing AMD — beating 1.00 needs a different family
(nested dissection) on the structured/grid-like matrices.

## Links
- Literature: _(add Amestoy-Davis-Duff 1996 note)_
- Compare: [nested-dissection.md](nested-dissection.md)

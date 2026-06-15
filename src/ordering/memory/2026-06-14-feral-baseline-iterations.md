# 2026-06-14 — iterating against the feral-AMD baseline

Baseline = `ssi_scoring::amd_baseline` = feral `amd_order` (real, supervariable
AMD). Score = geomean Σc²(yours)/Σc²(AMD) over 216 dev matrices. Starter
(natural) = 15.908.

## iter 1 — port memory-demo ND+AMD hybrid: score 1.0757 (fill 1.0328)
Ported the demo, but its O(d²) clique-fill `order_amd` blew the 5 s cap on
ampl (n=83k, 19.6 s). Replaced it with a near-linear **quotient-graph AMD**
(element absorption, approximate external degree, lazy degree buckets).

Per-family ratio (yours/AMD):
- bratu (40):      1.000  — banded PDE, AMD already optimal, ND finds same
- optctrl (42):    1.000  — staircase, zero extra fill either way
- rosenbrock (42): 1.000  — ~tridiagonal Hessian, zero fill both
- poisson (35):    1.2–1.5 — routed to ND, **LOSES to AMD**
- sparseqp (42):   ~1.20 (flat) — QG-AMD path, ~20% worse than feral AMD
- ampl (14):       0.9–1.65 mixed

### Diagnosis
The 118 banded matrices tie at 1.0 (no headroom). The drag is entirely
poisson + sparseqp. The flat sparseqp 1.20 is the tell: my QG-AMD lacks
**supervariable / indistinguishable-node detection** (feral AMD has it), so it
is ~20% worse on irregular graphs. poisson is routed to ND, but ND leaves and
separators are ordered by the same QG-AMD, so the structural ND gain on 2D
grids is canceled by the 20% leaf penalty → net loss.

Root cause for BOTH lossy families = QG-AMD quality. Fixing it lifts sparseqp
toward 1.0 and unmasks the real ND gain on poisson.

## iter 2 — QG-AMD + supervariable (mass-elimination) detection: 1.0532 (fill 1.0271)
Added supervariable detection: after rebuilding Lp members' adjacency, group by
a hash of (adjacent vars, adjacent elements), confirm exact equality, merge
duplicates (nv weight + member list), carry nv through the weighted
approximate-degree formula. Deterministic (sort-based grouping).

Effect: poisson improved markedly (k90 1.351→1.224, k85 1.367→1.179) because
ND leaves use this AMD. **sparseqp unchanged — still flat 1.200** (flops
nearly identical to iter1). Supervariables barely fire on sparseqp; its 20%
gap to feral-AMD is a different cause (tie-break / elimination-order shape,
since same total fill but lumpier Σc²).

### Key realization about the ceiling
The 118 bratu/optctrl/rosenbrock matrices are structurally banded → AMD is
optimal and I tie at 1.000; no headroom there. poisson are 2D 5-point grids
(avg deg ≈4), where ND's edge over AMD is only a small constant — so beating
feral-AMD overall (<1.0) is hard. Realistic target: drive poisson and sparseqp
toward 1.0.

## iter 3 — supervariable AMD as the DEFAULT, ND disabled: 0.9912 (fill 0.9974) ★ first sub-1.0
Diagnostic (force AMD for all) revealed the BFS-separator ND was hurting
EVERYWHERE, not just poisson. Crucially, `is_grid_like` was also catching
**sparseqp** (avg deg ≈3.3, max ≤6) and routing it to ND — that was the source
of the flat 1.20, NOT an AMD weakness. With ND off:
- poisson: 0.85–0.98 (my AMD BEATS feral AMD on 2D grids)
- sparseqp: 1.000 (ties feral AMD — banded/staircase KKT)
- ampl/gaslib40: 0.80 (supervariables win big)
- bratu/optctrl/rosenbrock: still 1.000
Set `ND_ENABLED=false`. Kept arrow deferral + ND code for a possible future
high-quality (multilevel) separator.

### Lesson
The whole premise inherited from memory (ND beats AMD on grids) was measured
against a RETIRED exact-MD baseline. Against the real feral-AMD baseline, a
good supervariable AMD already wins on these corpora; my hand-rolled BFS ND
is strictly worse. Don't re-enable ND without a genuinely better separator.

## Where the remaining loss is (from iter3 per-matrix)
- Large poisson 2D grids: 0.88–0.93 (already winning, but a real multilevel ND
  could in theory reach ~0.7 — modest geomean impact, ~10 matrices).
- Small poisson (k≤21): some LOSE, 1.03–1.13 — tiny n, cheap to fix with a
  richer tie-break given the 5 s budget is ~entirely unused (<30 ms/matrix).
- 118 banded ties at 1.000 — no headroom.

## iters 4–8 — selection / tie-break experiments: ALL no-improvement (best stays 0.9912)
The plain LIFO bucket-pop tie-break in the supervariable AMD is robustly the
best on this corpus. Every variation tried lost:
- iter 4: tie-break by fewest adjacent elements → 1.0162; smallest index → 1.0119
- iter 5: exact AMD approx-degree (min of bound2 & bound3) → 0.9915 (neutral-worse)
- iter 6: tie-break prefer heaviest supervariable (nv) → 1.0136
- iter 7: exact min-deg + min-fill for small subgraphs (ns≤2500) → 1.0002
  (perturbs the many small banded 1.000 ties; net loss)
- iter 8: disable arrow special-case → 0.9912 (neutral; arrow never fires on
  dev). Re-enabled as a cheap safeguard for hidden eval hub matrices.
Why LIFO wins: after a pivot, its neighbors are pushed last and popped first,
so elimination stays spatially local (low fill) on these mesh/banded graphs.

## STOPPED: 5 consecutive no-improvement iterations (4–8). Best = 0.9912.

## FINAL state of src/ordering/
- Supervariable quotient-graph AMD is the whole ordering (near-linear, safe to
  n=160k; largest matrix ~70 ms). LIFO tie-break. Third ADD degree bound only.
- `ND_ENABLED=false`, `ARROW_ENABLED=true` (arrow neutral but kept as guard).
- score 0.9912, fill 0.9974, all `cargo test` green, no warnings.

## Per-family at 0.9912 (from final run)
- ampl: 0.80–1.06 (supervariables win big on gaslib40)
- poisson (2D grids): ~0.85–0.98 large; a few small/mid LOSE (k16 1.13,
  k53 1.12, k90 1.07) — the residual drag above sub-0.99.
- bratu / optctrl / rosenbrock: 1.000 (banded/staircase; AMD optimal, no headroom)
- sparseqp: 1.000 (ties feral AMD)

## What I'd try next (open leads)
- A genuine MULTILEVEL nested dissection (coarsen → bisect coarse → project +
  FM refine) for the poisson 2D grids. The hand-rolled BFS-separator ND was
  strictly worse than AMD; only a real METIS-style separator could push poisson
  toward ~0.7. ~35 poisson matrices → meaningful geomean impact. Highest
  potential, highest effort; must stay under 5 s at n=160k.
- Constrained/partial min-fill ONLY on the poisson family (detect by degree
  regularity) so it can't regress the banded 1.000 ties (iter 7 failed because
  it was global).
- External-minimum-degree (true external degree vs approximate) — feral uses
  approximate; exact external degree occasionally helps grids.

# Harness Design — Local Prototype → Production Grader

This document explains how the test harness in this repository works, how an
AI agent (or human) uses it as a feedback loop, and what changes between this
local prototype and the production grader of `COMPETITION-PROPOSAL.md`.

---

## 1. Design principles (inherited from the proposal)

1. **The contestant submits an algorithm, not a score.** The submission is
   one function, `ordering::order(&Pattern) -> Vec<usize>`. The harness
   recomputes everything from that permutation; there is no number the
   contestant could fake.
2. **Narrow input.** The function sees the sparsity pattern only — no
   values, no right-hand side, no labels. The "answer" does not exist in
   the contestant's address space.
3. **Correctness is decoupled.** Any bijection of `0..n` factors correctly;
   the contestant can only move the cost. This makes the whole search space
   safe for unattended agents to explore.
4. **Fail loudly, never silently.** Invalid permutation, panic,
   nondeterminism, or a time-cap violation fails the entire run with a
   one-line reason. No partial credit, no silent fallback an agent could
   accidentally rely on.
5. **Determinism end to end.** The corpus is generated from fixed seeds, the
   scorer is a pure function, and the contestant code is run twice and
   required to agree — so the same submission scores identically on every
   machine, every time.

## 2. Architecture

```
ssi-ordering-challenge/
├── Cargo.toml            CONTRACT: zero dependencies, stable Rust
├── src/
│   ├── main.rs           harness driver (stages, caps, scoring, output)   [frozen]
│   ├── pattern.rs        Pattern type + deterministic corpus generators   [frozen]
│   ├── symbolic.rs       trusted scorer: etree + column counts → flops    [frozen]
│   ├── baseline.rs       frozen minimum-degree baseline + validator       [frozen]
│   └── ordering/         ★ contestant code — the only editable directory
│       ├── mod.rs        pub fn order(&Pattern) -> Vec<usize>
│       └── memory/       agent notes across iterations
├── results.tsv           append-only run log (timestamp, status, score, note)
├── score.json            latest score, machine-readable
└── docs/HARNESS-DESIGN.md
```

Per matrix, `main.rs` executes the five grader stages of the proposal in
miniature:

| Proposal stage | Prototype implementation |
|---|---|
| A — purity & license gate | enforced socially here (`[dependencies]` empty); enforced mechanically in production (`cargo-deny`, no-FFI scan, offline vendored registry) |
| B — sandboxed compile & run | plain `cargo run` here, with the 10 s/matrix **time cap** enforced in-process; production adds a no-network/no-filesystem sandbox and a 2–4 GB memory cap |
| C — output validation | `validate_permutation`: exact bijection check; panics caught via `catch_unwind` |
| D — trusted scoring | `symbolic::analyze`: elimination tree (Liu 1986, path compression) + O(nnz(L)) row-subtree column counts → `nnz(L)` and `flops = Σ cⱼ²`, computed from the permutation alone |
| E — reproducibility | the ordering is run twice per matrix; outputs must be identical |

**Score** = geometric mean over the corpus of
`flops(contestant) / flops(baseline)`, tie-broken by the geomean fill ratio.
The geometric mean keeps any single matrix (e.g. the arrow's 148,366×
blowup) from dominating: it contributes one log-term like every other
matrix.

### The trusted scorer in one paragraph

Given pattern A and permutation P, the scorer materializes the permuted
pattern B = PAPᵀ (O(nnz)), builds the elimination tree of B with ancestor
path compression, then computes the exact per-column nonzero counts of the
Cholesky/LDLᵀ factor L by the row-subtree traversal: L(i,k) ≠ 0 exactly when
k lies on the etree path from some j (B(i,j) ≠ 0, j < i) up to i. Total work
O(nnz(L)) — 1–2 orders of magnitude cheaper than the numeric factorization
it predicts, which is what makes grading cheap (cf.
`COMPETITION-VERIFIER-COST.md`: ~ms per matrix, seconds per full run). The
flop proxy Σⱼ cⱼ² is a deterministic, hardware-independent stand-in for the
LDLᵀ operation count; since the score is a *ratio* against the baseline
under the same model, the model's constant factors cancel.

### The corpus

Generated deterministically at startup (no data files, no network):

- **2D grids** (30², 60², 90² five-point Laplacians) and **3D grids**
  (10³, 14³ seven-point) — where nested dissection provably wins and
  minimum degree is known to be suboptimal: the headroom is real and
  certified by theory (George 1973: O(n log n) fill vs MD's worse behavior).
- **Saddle-point KKT patterns** ([[H Aᵀ],[A 0]] with banded local coupling,
  three sizes) — the indefinite structure the production corpus consists of;
  the zero (2,2) block punishes naive orderings.
- **The arrow** — the canonical showcase and a tripwire: any ordering that
  mishandles dense rows is instantly visible.

## 3. The agent feedback loop

The loop an agent runs is exactly the ecdsa.fail loop:

```
edit src/ordering/  →  cargo run --release -- --note "hypothesis"
       ↑                                 │
       │      per-matrix table + score + FAIL reasons
       └─────────────────────────────────┘
              write findings to src/ordering/memory/
```

Three properties of the harness make this loop productive for an agent:

1. **Per-matrix attribution.** The table shows the ratio per matrix, so an
   agent sees *where* a change helped (e.g. "0.76 on grid2d_90, 1.22 on
   grid3d_14") and can form family-specific hypotheses, not just watch one
   scalar.
2. **Failure messages are actionable.** "`kkt_2000_700: ordering took
   14.69s, cap is 10s`" or "`index 5 appears more than once`" point directly
   at the fix. This was validated live: the demo trajectory below includes a
   real cap violation and its repair.
3. **Cheap iterations.** A full run is < 1 s of scoring plus the contestant's
   own ordering time; the Rust compile dominates. Hundreds of iterations per
   hour are realistic.

### A real four-iteration trajectory (recorded in `results.tsv`)

| iter | change | score | feedback used |
|---|---|---|---|
| 0 | starter: natural ordering | **42.48** | arrow ratio 148,366× ⇒ dense-row handling matters |
| 1 | reverse Cuthill–McKee | **1.64** | grids improved most; KKTs still ≫1 ⇒ bandwidth ≠ fill |
| 2 | MD + exact min-fill tie-break | **FAIL** | `grid3d_14: ordering took 26.05s, cap is 10s` — O(d²) per candidate blows up as 3D degrees grow |
| 3 | tie-break only while min degree ≤ 12 | **0.9496** | beats the baseline: grid2d_90 0.593, KKTs 0.93–0.97; still loses on grid3d_14 (1.20) and kkt_4000_1500 (1.06) ⇒ next: nested dissection |

The full notes live in `src/ordering/memory/2026-06-10-demo-trajectory.md`
as a worked example of the memory convention.

## 4. Anti-cheat analysis of the prototype

| attack | defense |
|---|---|
| report a fake score | impossible — contestant returns a permutation; the frozen scorer derives the score |
| return a malformed/partial permutation hoping for leniency | Stage C bijection check fails the run |
| nondeterministic ordering that "gets lucky" | Stage E double-run equality check |
| stall/explore unboundedly | 10 s/matrix cap, enforced and demonstrated |
| read the answer / RHS | doesn't exist: the input type carries pattern only |
| hardcode permutations for the corpus | works locally by design (it's the *dev* corpus); defeated in production by the hidden, per-round-regenerated eval corpus + memory cap |
| edit the harness/baseline | local honor system; production grader rebuilds the harness from its own trusted copy and only takes `src/ordering/` from the submission |
| escape to non-Rust code / deps | `[dependencies]` empty here; production Stage A rejects build scripts, `*-sys` crates, FFI, and non-permissive licenses, and builds offline |

## 5. Path to the production grader

What changes, in order of importance — nothing about the contract changes:

1. **Swap the scorer**: replace `symbolic.rs` with a call into feral's
   symbolic analysis (`symbolic_factorize`), so the score is feral's own
   predicted flop/fill model — the one the leaderboard advertises. The
   `feral-grader` binary of the proposal is this harness with that one
   substitution plus corpus loading.
2. **Swap the baseline**: `baseline.rs`'s exact minimum degree → feral's
   AMD. (Exact MD is the algorithm AMD approximates; scores shift slightly
   but the anchor stays 1.00 by construction.)
3. **Swap the corpus**: synthetic generators → a stratified ~500-matrix
   hidden slice of feral's ~183k IPM-harvested KKT corpus (4 size buckets ×
   family, per `COMPETITION-VERIFIER-COST.md`), with a disjoint public dev
   slice shipped in the starter template, and per-round fresh regeneration
   (patterns only — no reference labeling needed).
4. **Harden the sandbox**: contestant crate compiled offline against a
   vendored registry, run with no network/filesystem, 2–4 GB memory cap
   (which doubles as the anti-lookup-table cap), `cargo-deny` license gate,
   static no-FFI scan.
5. **Wire the leaderboard**: PR/upload → CI runs stages A–E → one number
   posted; anchors AMD = 1.00 with METIS-style nested dissection and MUMPS
   fill as reference lines. Submission UX mirrors ecdsa.fail (CLI: clone /
   run / submit; `score.json` and `results.tsv` formats are already
   compatible).

Per `COMPETITION-VERIFIER-COST.md`, the production economics hold:
compile-dominated ~3–5 min/submission, embarrassingly parallel scoring,
fractions of a cent per submission.

## 6. Known prototype limitations (deliberate)

- The flop model is Σ cⱼ² rather than feral's exact LDLᵀ count — fine for a
  ratio metric, replaced wholesale in step 1 above.
- The baseline exact-MD is O(n) scan per pivot and clique-based — adequate
  for the dev corpus sizes (≤ 8.1k), not for 10⁵-row production matrices;
  replaced in step 2.
- Supernode amalgamation (the proposal's optional second seam) is not
  modeled; it arrives for free with feral's symbolic engine in step 1.
- The synthetic KKT generator uses banded locality; real IPM KKTs have
  richer structure — replaced in step 3.

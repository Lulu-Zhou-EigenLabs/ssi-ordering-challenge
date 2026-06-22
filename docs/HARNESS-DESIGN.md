# Harness Design — Contestant Template & Production Grader

This document explains how the harness in this repository works, how an AI
agent (or human) uses it as a feedback loop, and how it relates to the
production grader of `COMPETITION-PROPOSAL.md`. As of Phase 3 the harness is
**feral-backed**: the score and the AMD baseline are computed by feral's own
symbolic analysis, via the `ssi-scoring` crate that the grader also uses.

---

## 1. Design principles (inherited from the proposal)

1. **The contestant submits an algorithm, not a score.** The submission is
   one function, `ordering::order(&Pattern) -> Vec<usize>`. The harness
   recomputes everything from that permutation; there is no number the
   contestant could fake.
2. **Narrow input.** The function sees the sparsity pattern only — no
   values, no right-hand side, no labels. The `Pattern` type has no field for
   values; the loader drops them. Asserted in `tests/narrow_input.rs`.
3. **Correctness is decoupled.** Any bijection of `0..n` factors correctly;
   the contestant can only move the cost. This makes the whole search space
   safe for unattended agents to explore.
4. **Fail loudly, never silently.** Purity/license violation, invalid
   permutation, panic, nondeterminism, or a time-cap violation fails the
   entire run with a one-line reason. No partial credit, no silent fallback.
5. **One scoring path.** The harness and the grader both score by calling the
   same functions in `ssi-scoring`. The score is a pure function of
   `(pattern, permutation)`, so a local score equals the graded score for the
   same ordering on the same matrices (exact-grader equivalence).

## 2. Architecture

```
ssi-ordering-challenge/            (THE PUBLIC REPO / contestant template)
├── Cargo.toml            workspace root; harness package + members          [frozen]
├── deny.toml             shared license policy (cargo-deny)                  [frozen]
├── src/
│   ├── main.rs           harness driver (gate, stages, caps, scoring, output)[frozen]
│   ├── pattern.rs        Pattern re-export + dev-corpus loader (JSONL)        [frozen]
│   ├── purity.rs         local Stage-A purity & license gate                 [frozen]
│   └── ordering/         ★ contestant code — the only editable directory
│       ├── mod.rs        pub fn order(&Pattern) -> Vec<usize>  (starter stub)
│       └── memory/       agent notes + a reference ND+AMD demo
├── ssi-scoring/          THE SCORING WRAPPER (trusted; also used by grader)  [frozen]
│   └── src/
│       ├── lib.rs        score(), amd_baseline() via feral building blocks
│       ├── pattern.rs    the Pattern type (structure only)
│       └── loader.rs     pattern_from_jsonl_line() + load_pattern() (.mtx); shared by harness & grader
├── prototype-oracle/     dev-only INDEPENDENT scorer, for the cross-check test
├── corpus/dev/           the shipped development corpus (patterns.jsonl sample)
├── results.tsv           append-only run log (timestamp, status, score, note)
├── score.json            latest score, machine-readable
└── docs/HARNESS-DESIGN.md
```

Per run, `main.rs` executes the grader stages of the proposal in miniature:

| Proposal stage | Harness implementation |
|---|---|
| A — purity & license gate | `purity::check`: scans `src/ordering/` for build.rs / FFI / `#[no_mangle]` / `#[link]` / proc-macros / `include!` escapes / added dependencies, and runs `cargo-deny` against `deny.toml` (fallback to a dependency scan if cargo-deny is absent). Mirrors the grader's authoritative Stage A — same rules, same `deny.toml`. |
| B — sandboxed compile & run | `cargo run` here runs each `order()` in a child process (`--worker` mode) supervised by a watchdog (`src/watchdog.rs`) that SIGKILLs it at the **2 s/matrix time cap** — the same enforcement mechanism the grader uses; the production grader additionally adds a no-network/no-filesystem sandbox and a 2–4 GB memory cap |
| C — output validation | `validate_permutation`: exact bijection check; panics caught via `catch_unwind` |
| D — trusted scoring | `ssi_scoring::score`: feral's pattern-pure building blocks — `symmetric_pattern → permute_pattern → EliminationTree::from_pattern → column_counts_gnp`, then `nnz(L) = Σ cⱼ` and `flops = Σ cⱼ²`, computed from the permutation alone |
| E — reproducibility | the ordering is run twice per matrix; outputs must be identical |

**Score** = geometric mean over the corpus of
`flops(contestant) / flops(AMD)`, tie-broken by the geomean fill ratio.
The geometric mean keeps any single matrix from dominating: it contributes one
log-term like every other matrix.

### The trusted scorer in one paragraph

`ssi-scoring` is the only code in the workspace that calls feral. Given a
pattern A and permutation P it does **not** call `feral::symbolic_factorize`
(whose default ordering is AMF/MetisND and whose `LdltCompress` preprocess can
read matrix *values* via MC64 matching — Phase 1 R5). Instead it composes
feral's already-public, pattern-only building blocks, which is feral's own fill
computation: permute the full-symmetric pattern, build the elimination tree
(Liu 1986), compute exact per-column counts of L by Gilbert–Ng–Peyton, and sum
to `nnz(L) = Σ cⱼ` and `flops = Σ cⱼ²`. Supernode amalgamation never enters
(counts are computed before it, and fill is invariant under its within-subtree
relabeling — Phase 1 §2), so there is no amalgamation knob to tune. Both the
contestant permutation and the AMD baseline (`feral_amd::amd_order`, default
options, pattern-pure) go through this identical path. Symbolic analysis is
1–2 orders of magnitude cheaper than the numeric factorization it predicts,
which is what makes grading cheap (cf. `COMPETITION-VERIFIER-COST.md`).

### The corpus

`corpus/dev/patterns.jsonl` — real KKT / saddle-point patterns harvested from
interior-point solves, stratified across size buckets and the four problem
families (NLP / QCP / QP / QCQP). Each line is one symmetric sparsity pattern in
compressed-sparse-column form (`n`, `indptr`, `indices`, plus `hash`/`source`
metadata) — structure only, no values anywhere (NARROW INPUT holds by
construction). The stored pattern includes the diagonal; the shared reader
`ssi_scoring::pattern_from_jsonl_line` drops it to produce the off-diagonal
contract `Pattern`. **What ships in the repo is a small sample** (13 matrices)
for pipeline smoke-testing; the full corpus (~279 patterns, n up to ~340k) is
published for download (see `corpus/dev/README.md`). The grader scores a
disjoint, hidden evaluation slice from the same distribution. The synthetic
generators of the prototype (`arrow`, `grid2d`, `grid3d`, `kkt`) survive only
as `cargo test` fixtures in the `prototype-oracle` crate.

## 3. The agent feedback loop

```
edit src/ordering/  →  cargo run --release -- --note "hypothesis"
       ↑                                 │
       │      per-matrix table + score + FAIL reasons
       └─────────────────────────────────┘
              write findings to src/ordering/memory/
```

The per-matrix table shows the flop ratio per matrix, so an agent can attribute
wins/losses by family (KKT size buckets, the analytic families) and form
targeted hypotheses. Failure messages name the offending matrix and reason
(cap violation, invalid permutation, purity hit), pointing directly at the fix.

## 4. Anti-cheat analysis

| attack | defense |
|---|---|
| report a fake score | impossible — contestant returns a permutation; feral's scorer derives the score |
| return a malformed/partial permutation | Stage C bijection check fails the run |
| nondeterministic ordering that "gets lucky" | Stage E double-run equality check |
| stall/explore unboundedly | 2 s/matrix cap, enforced via SIGKILL in a child process |
| read the answer / RHS | doesn't exist: `Pattern` carries structure only; loader drops values |
| hardcode permutations for the corpus | works locally by design (it's the *dev* corpus); defeated in production by the hidden, per-round-regenerated eval corpus + memory cap |
| edit the harness/scorer/baseline | local honor system; the grader rebuilds the harness and `ssi-scoring` from its own trusted copy and takes only `src/ordering/` from the submission |
| escape to non-Rust code / deps | `purity::check` rejects build scripts, FFI, `#[no_mangle]`/`#[link]`, proc-macros, and added dependencies in `src/ordering/`; `cargo-deny` rejects non-permissive licenses; the production grader builds offline against a vendored registry |

## 5. What still differs in the production grader

Nothing about the contract, the metric, the baseline, or the scoring path
changes — those are shared via `ssi-scoring`. The grader adds:

1. **The hidden eval corpus**: a disjoint, stratified, per-round-regenerated
   slice from the same IPM-harvested distribution (Phase 2 built the pipeline;
   `grader/` holds it). Patterns only — no reference labeling needed.
2. **The sandbox**: contestant crate compiled offline against a vendored
   registry, run with no network/filesystem, 2–4 GB memory cap (which doubles
   as the anti-lookup-table cap).
3. **The leaderboard wiring**: PR/upload → CI runs stages A–E → one number
   posted; anchors AMD = 1.00 with METIS-style nested dissection and MUMPS
   fill as reference lines.

The local purity/license gate (`purity.rs` + `deny.toml`) is the same code and
policy the grader's Stage A uses, so a submission that passes locally passes the
server's gate.

## 6. Verification built into `cargo test`

- **Closed-form scorer tests** (`ssi-scoring`): dense 3×3 → flops 14;
  tridiagonal → nnz(L) = 2n−1; arrow hub-first → n(n+1)/2; hub-last →
  near-zero fill. Mathematical facts (Invariant 4), ported to the feral scorer.
- **Scorer cross-check** (`tests/scorer_crosscheck.rs`): the feral scorer and
  the independent `prototype-oracle` scorer agree exactly on `nnz(L)` and
  `flops` across grids, 3D grids, KKTs, and the arrow, under identity, reverse,
  and AMD orderings.
- **One reader, no cross-check needed**: the harness and grader parse a corpus
  line into a `Pattern` through the *same* `ssi_scoring::pattern_from_jsonl_line`,
  so there is no second parser that could silently disagree — Invariant 2 holds
  at the parsing boundary by construction, not by an agreement test. (The former
  `tests/loader_agreement.rs`, which cross-checked two `.mtx` parsers, is
  retired with the dual-reader design.)
- **Exact equivalence** (`tests/exact_equivalence.rs`): pinned `(nnz_l, flops)`
  for the identity ordering on three committed sample matrices (`st_e09`,
  `ex8_5_2`, `gilbert`), so any drift between local and grader scoring breaks
  the build immediately. `gilbert` (n=1001) is a hub-last arrow/star under
  natural order (leaf columns cost 2, the hub 1 — zero fill), so its pin
  `nnz_l = 2·1000 + 1 = 2001` is a closed-form check the numbers are genuine.
- **Narrow input** (`tests/narrow_input.rs`): different values, identical
  pattern ⇒ identical score; the `.mtx` value column is never consulted (and the
  JSONL corpus carries no values at all).

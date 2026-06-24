# End-to-End Workflow

This document traces what actually *happens*, step by step, when you run this
repository — from the command line, through every harness stage, into the
scoring wrapper, and back out to the files on disk. Where `HARNESS-DESIGN.md`
explains *why* the pieces exist, this explains *how the bytes flow* and points
at the exact code for each step.

The whole flow is driven by one command:

```
cargo run --release -- --note "what I tried"
```

which runs `fn main()` in [`src/main.rs`](../src/main.rs).

---

## 0. Inputs and outputs at a glance

| | |
|---|---|
| **You edit** | `src/ordering/` — only this directory (the rest is frozen harness). |
| **The harness reads** | every pattern in `corpus/dev/patterns.jsonl` (a small in-repo sample; full corpus downloads separately). |
| **The harness writes** | `score.json` (latest score) and one appended row in `results.tsv`. |
| **The score** | geomean over the corpus of `flops(yours) / flops(AMD)`, lower is better, AMD anchored at 1.00. Tiebreak: geomean of `nnz(L)(yours) / nnz(L)(AMD)`. |

---

## 1. The data: what an input matrix is

The corpus is one file, `corpus/dev/patterns.jsonl`: one JSON object per line,
each a symmetric sparsity pattern in compressed-sparse-column (CSC) form —
`{"n", "nnz", "indptr", "indices", "hash", "source"}`. There are **no values
anywhere** — the corpus is pattern-only by construction, so by the time your
code sees a matrix it is a pure graph.

One reader parses a line into a `Pattern`:
`ssi_scoring::pattern_from_jsonl_line`
([`ssi-scoring/src/loader.rs`](../ssi-scoring/src/loader.rs)). The harness loads
the whole sample with `load_corpus_jsonl`; the grader can load one line by index
with `load_pattern_jsonl_line` — **both route through the same parse core**, so
there is no second parser that could silently disagree (Invariant 2 at the
parsing boundary). The stored pattern is the full symmetrized pattern and
**includes the diagonal**; the reader drops `i == j` to produce the off-diagonal
contract `Pattern`.

The type your `order()` receives is `Pattern`
([`ssi-scoring/src/pattern.rs`](../ssi-scoring/src/pattern.rs:23)), re-exported
through `src/pattern.rs` so the signature is identical on both sides:

```rust
pub struct Pattern {
    pub n: usize,            // dimension
    pub col_ptr: Vec<usize>, // length n+1, CSC column pointers
    pub row_idx: Vec<usize>, // row indices, concatenated, sorted+unique per column
}
```

It carries the **full symmetric** off-diagonal pattern (both triangles), no
values, no right-hand side. Use `pattern.col(j)` for the neighbor list of vertex
`j` and `pattern.nnz()` for the off-diagonal nonzero count
([`ssi-scoring/src/pattern.rs:61`](../ssi-scoring/src/pattern.rs:61)).

---

## 2. The harness run, stage by stage

`main()` executes the five grader stages of the proposal in miniature. Any
failure at any stage prints a one-line reason, appends a `FAIL` row to
`results.tsv`, and exits non-zero — no partial credit.

### Stage A — purity & license gate (before any scoring)

[`src/main.rs`](../src/main.rs) calls `purity::check(&repo_root)`, a thin
delegator ([`src/purity.rs`](../src/purity.rs:15)) to the shared `ssi-purity`
crate. It scans `src/ordering/` for anything that escapes stdlib: added
dependencies, `build.rs`, FFI / `extern`, `#[no_mangle]` / `#[link]`,
proc-macros, or `include!` reaching outside the directory. The local harness
runs in `FallbackAllowed` mode; the grader runs the *same crate* in
`RequireDeny` mode, so a submission that passes locally passes the server gate.
A violation here fails the run before a single matrix is scored.

### Load the corpus

[`src/main.rs`](../src/main.rs) calls `pattern::dev_corpus_indexed()`
([`src/pattern.rs`](../src/pattern.rs)), which loads every pattern in
`corpus/dev/patterns.jsonl` in file order via the shared
`ssi_scoring::load_corpus_jsonl` reader, tagging each with its 0-based raw line
index so the parent can hand the worker subprocess the exact line to load. An
empty or missing corpus aborts the run (run from the repo root).

### Per matrix (the loop at [`src/main.rs`](../src/main.rs))

For each `(raw_index, name, pattern)`:

1. **AMD baseline** — [`src/main.rs`](../src/main.rs):
   `ssi_scoring::amd_baseline(pat)` runs feral's AMD, then
   `score(pat, &base_perm)` computes its `(nnz_l, flops)`. This is the 1.00
   anchor for this matrix.

2. **Stage B — run your `order()`, twice, in a killable subprocess**
   ([`src/main.rs`](../src/main.rs), `run_once`): each call re-launches this
   same binary in `--worker` mode (passing the matrix's raw line index), which
   loads the pattern, runs `order()`, and writes the permutation to a scratch
   file. The parent supervises that child with the watchdog
   ([`src/watchdog.rs`](../src/watchdog.rs)):
   - if the child exceeds `TIME_CAP_PER_MATRIX = 2s` it is **SIGKILLed** and the
     run fails for a **cap violation** — enforced, not merely measured;
   - if the child exits non-zero or writes no permutation (e.g. a **panic** in
     `order()`) the run fails;
   - `order()` is run a **second time**; if `perm1 != perm2` the run fails for
     **nondeterminism** (Stage E analog).

3. **Stage C — output validation** — `validate_permutation(&perm1, pat.n)`
   ([`src/main.rs`](../src/main.rs)) checks the result is a true
   bijection of `0..n` (right length, every index in range, no duplicates).

4. **Stage D — trusted scoring** — [`src/main.rs`](../src/main.rs):
   `score(pat, &perm1)` — **the same function** that scored the baseline. Your
   code never reports a number; the harness derives it. The per-matrix ratio is
   `yours.flops / base.flops`; logs of the ratio and the fill ratio accumulate
   ([`src/main.rs`](../src/main.rs)).

A row is printed per matrix: `name n nnz(A) flops(base) flops(yours) ratio time`.

### Aggregate and emit

After the loop ([`src/main.rs`](../src/main.rs)):

- On any failure: print the reason, append a `FAIL` row, exit 1.
- On success: `score = exp(mean(ln ratio))`, `fill = exp(mean(ln fill_ratio))`
  ([`src/main.rs`](../src/main.rs)); write `score.json` and append an
  `OK` row to `results.tsv` (`timestamp  status  score  fill  note`, via
  `append_results`, [`src/main.rs`](../src/main.rs)).

The geometric mean keeps any single matrix from dominating — each contributes
one log-term.

---

## 3. How the score is computed (`ssi-scoring`)

`ssi-scoring` is the **only** code in the repo that calls feral, and both the
baseline and your ordering go through it — this is the "one scoring path"
invariant that makes a local score equal the graded score.

`score(pattern, perm)` ([`ssi-scoring/src/lib.rs:86`](../ssi-scoring/src/lib.rs:86))
is a pure function of `(pattern, permutation)`. It deliberately avoids
`feral::symbolic_factorize` (whose default ordering can read matrix *values*)
and instead composes feral's pattern-only building blocks:

```
Pattern → CscPattern
  → permute_pattern(pattern, P)        // apply your permutation
  → EliminationTree::from_pattern      // Liu 1986
  → column_counts_gnp                  // Gilbert–Ng–Peyton, exact column counts c_j
  → nnz_l = Σ c_j                      // fill — the tiebreak
    flops = Σ c_j²                     // the deterministic Σc² flop model — the score
```

`amd_baseline` ([`ssi-scoring/src/lib.rs:108`](../ssi-scoring/src/lib.rs:108))
pins the baseline to `feral_amd::amd_order` with default options on the raw
full-symmetric pattern — deterministic and pattern-pure, reproducible from a
pattern file alone.

These are mathematical facts, locked by the closed-form tests in
[`ssi-scoring/src/lib.rs:136`](../ssi-scoring/src/lib.rs:136) (dense 3×3 →
flops 14; tridiagonal → nnz(L) = 2n−1; arrow hub-first → n(n+1)/2; hub-last →
near-zero fill).

---

## 4. What the submission does today (`src/ordering/mod.rs`)

The frozen contract is one function
([`src/ordering/mod.rs`](../src/ordering/mod.rs)):

```rust
pub fn order(pattern: &Pattern) -> Vec<usize>
```

`perm[k]` = the original index eliminated `k`-th. **What ships is a starter
stub:** `order()` returns the identity / natural order, `(0..n).collect()`. It
is a valid, deterministic permutation (so the harness runs end to end), but it
is deliberately *not* a good ordering — on real KKT patterns the identity
eliminates dense constraint rows early and densifies the factor, scoring far
worse than the AMD baseline. It is the starting line, not a solution.

Your job is to replace it. Anything under `src/ordering/` is yours: rewrite the
file, split it into submodules, add helpers — as long as `order()` keeps its
signature and stays a deterministic bijection within the 2 s cap. Cost scales
with **density (nnz, max-degree)**, not just `n`, so gate any expensive path by
both. The classic ordering families to study — minimum-degree / AMD, nested
dissection, minimum-fill, local-search refinement — are in `README.md` →
"Background reading"; record what you try in `src/ordering/memory/`.

> **Tip — a private cost estimate.** A useful pattern is to *predict* the score
> of a candidate ordering yourself (the metric is `Σ c_j²` over the column
> counts of L; see §3) so you can generate several candidates and keep the best.
> That is purely your own logic — the official score is always recomputed by
> `ssi_scoring::score` from the permutation you return, so any estimator you
> build affects only *which* ordering you choose, never how it is scored.

---

## 5. The development loop

```
read results.tsv + src/ordering/memory/
        │
        ▼
edit src/ordering/      ──►   cargo run --release -- --note "hypothesis"
        ▲                              │
        │        per-matrix table + score + any FAIL reason
        └──────────────────────────────┘
        write findings to src/ordering/memory/ ; commit when the score improves
```

The per-matrix table lets you attribute wins/losses by family (`NLP`, `QCP`,
`QP`, `QCQP`) and size bucket, then form a targeted hypothesis. `results.tsv` is
the append-only history of every run, so your score progression over time is
recorded there; keep the reasoning behind each step in `src/ordering/memory/`.

---

## 6. How this maps to the production grader

**The grader is this same harness binary**, run in the repo's own GitHub Actions
(`.github/workflows/benchmark.yml`) and dispatched by the Yukon platform. It
builds a candidate from the validated baseline + **only** the submission's
`src/ordering/`, runs the workflow, and reads the uploaded `score.json`. The
workflow grades a **hidden** eval corpus — disjoint from the dev corpus,
regenerated per round — injected via the `SSI_CORPUS_FILE` path override and
downloaded at run time to a temp path outside the repo tree (so the eval bytes
are never committed). Scoring runs in a sandbox (no network, no filesystem,
2–4 GB memory cap, determinism re-runs).

Because grading runs the identical harness + `ssi-scoring` functions you run
locally, the number you see locally is structurally the number the grader
reproduces for the same ordering — the design's exact-grader equivalence. What
grading adds is the hidden corpus, the sandbox, and the platform wiring (PR per
submission, dispatch, score comment, accept/close); the contract, the metric,
the baseline, and the scoring path are unchanged. See
[`HARNESS-DESIGN.md`](HARNESS-DESIGN.md) §4–§5 for the anti-cheat analysis and
the precise list of grader-only additions.

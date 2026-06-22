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

which runs `fn main()` in [`src/main.rs`](../src/main.rs:56).

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

[`src/main.rs:61`](../src/main.rs:61) calls `purity::check(&repo_root)`, a thin
delegator ([`src/purity.rs`](../src/purity.rs:15)) to the shared `ssi-purity`
crate. It scans `src/ordering/` for anything that escapes stdlib: added
dependencies, `build.rs`, FFI / `extern`, `#[no_mangle]` / `#[link]`,
proc-macros, or `include!` reaching outside the directory. The local harness
runs in `FallbackAllowed` mode; the grader runs the *same crate* in
`RequireDeny` mode, so a submission that passes locally passes the server gate.
A violation here fails the run before a single matrix is scored.

### Load the corpus

[`src/main.rs:68`](../src/main.rs:68) calls `pattern::dev_corpus()`
([`src/pattern.rs`](../src/pattern.rs)), which loads every pattern in
`corpus/dev/patterns.jsonl` in file order via the shared
`ssi_scoring::load_corpus_jsonl` reader. An empty or missing corpus aborts the
run (run from the repo root).

### Per matrix (the loop at [`src/main.rs:87`](../src/main.rs:87))

For each `(name, pattern)`:

1. **AMD baseline** — [`src/main.rs:89`](../src/main.rs:89):
   `ssi_scoring::amd_baseline(pat)` runs feral's AMD, then
   `score(pat, &base_perm)` computes its `(nnz_l, flops)`. This is the 1.00
   anchor for this matrix.

2. **Stage B — run your `order()`, timed and twice**
   ([`src/main.rs:93`](../src/main.rs:93)):
   - First call is wrapped in `catch_unwind` — a **panic** fails the run.
   - Elapsed time is checked against `TIME_CAP_PER_MATRIX = 2s`
     ([`src/main.rs:54`](../src/main.rs:54)) — a **cap violation** fails the run.
   - It is called a **second time**; if `perm1 != perm2` the run fails for
     **nondeterminism** (Stage E analog, [`src/main.rs:117`](../src/main.rs:117)).

3. **Stage C — output validation** — `validate_permutation(&perm1, pat.n)`
   ([`src/main.rs:172`](../src/main.rs:172)) checks the result is a true
   bijection of `0..n` (right length, every index in range, no duplicates).

4. **Stage D — trusted scoring** — [`src/main.rs:127`](../src/main.rs:127):
   `score(pat, &perm1)` — **the same function** that scored the baseline. Your
   code never reports a number; the harness derives it. The per-matrix ratio is
   `yours.flops / base.flops`; logs of the ratio and the fill ratio accumulate
   ([`src/main.rs:130`](../src/main.rs:130)).

A row is printed per matrix: `name n nnz(A) flops(base) flops(yours) ratio time`.

### Aggregate and emit

After the loop ([`src/main.rs:149`](../src/main.rs:149)):

- On any failure: print the reason, append a `FAIL` row, exit 1.
- On success: `score = exp(mean(ln ratio))`, `fill = exp(mean(ln fill_ratio))`
  ([`src/main.rs:156`](../src/main.rs:156)); write `score.json` and append an
  `OK` row to `results.tsv` (`timestamp  status  score  fill  note`, via
  `append_results`, [`src/main.rs:203`](../src/main.rs:203)).

The geometric mean keeps any single matrix from dominating — each contributes
one log-term.

---

## 3. How the score is computed (`ssi-scoring`)

`ssi-scoring` is the **only** code in the repo that calls feral, and both the
baseline and your ordering go through it — this is the "one scoring path"
invariant that makes a local score equal the graded score.

`score(pattern, perm)` ([`ssi-scoring/src/lib.rs:84`](../ssi-scoring/src/lib.rs:84))
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

`amd_baseline` ([`ssi-scoring/src/lib.rs:106`](../ssi-scoring/src/lib.rs:106))
pins the baseline to `feral_amd::amd_order` with default options on the raw
full-symmetric pattern — deterministic and pattern-pure, reproducible from a
pattern file alone.

These are mathematical facts, locked by the closed-form tests in
[`ssi-scoring/src/lib.rs:145`](../ssi-scoring/src/lib.rs:145) (dense 3×3 →
flops 14; tridiagonal → nnz(L) = 2n−1; arrow hub-first → n(n+1)/2; hub-last →
near-zero fill).

---

## 4. What the submission does today (`src/ordering/mod.rs`)

The frozen contract is one function
([`src/ordering/mod.rs:41`](../src/ordering/mod.rs:41)):

```rust
pub fn order(pattern: &Pattern) -> Vec<usize>
```

`perm[k]` = the original index eliminated `k`-th. The current implementation is
a **generate-candidates-and-keep-the-best** strategy. It builds an adjacency
list, then evaluates several candidate orderings and keeps whichever has the
lowest *predicted* `Σc_j²`:

- Supervariable quotient-graph **AMD** with LIFO and FIFO tie-breaks
  (`order_amd`, [`src/ordering/mod.rs:841`](../src/ordering/mod.rs:841));
- **Arrow-hub deferral** when a dominant hub is detected
  ([`src/ordering/mod.rs:65`](../src/ordering/mod.rs:65));
- Exact **min-fill** for small matrices, n ≤ 3000
  ([`src/ordering/mod.rs:86`](../src/ordering/mod.rs:86));
- **Seeded random-restart AMD**, budgeted to stay under the time cap
  ([`src/ordering/mod.rs:96`](../src/ordering/mod.rs:96));
- Nested dissection exists but is **disabled** — it never beat AMD on this
  corpus ([`src/ordering/mod.rs:19`](../src/ordering/mod.rs:19)).

The key enabler is `predict_flops`
([`src/ordering/mod.rs:267`](../src/ordering/mod.rs:267)): a pure-stdlib
reimplementation of the harness's exact `Σc_j²` metric (Davis/CSparse
elimination tree + column counts). Because the selector scores every candidate
with the *same* metric the harness uses, adding candidates can only lower or tie
the final score — never raise it. Selection ties keep the earlier candidate, so
the result stays deterministic for the twice-run gate.

> **Boundary:** `predict_flops` is the contestant's *private estimate* used only
> to pick a candidate. The official score is always recomputed by
> `ssi_scoring::score`. The two agree by construction (same algorithm), and the
> cross-check tests in `ssi-scoring` keep them honest — but if you change
> `predict_flops` you change only *which candidate is chosen*, never how it is
> scored.

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
the append-only history of every run; the best committed score to date is
recorded there and in `src/ordering/memory/`.

---

## 6. How this maps to the production grader

The grader (`grader/`, private, never published) repeats Stages A–E against a
**hidden** eval corpus that is disjoint from the dev corpus and regenerated per
round. It extracts **only** `src/ordering/` from a submission, drops it into its
own trusted copy of the harness, and scores inside a sandbox (no network, no
filesystem, 2–4 GB memory cap, determinism re-runs).

Because both sides score through the identical `ssi-scoring` functions, the
number you see locally is structurally the number the grader reproduces for the
same ordering — the design's exact-grader equivalence. What the grader adds is
the hidden corpus, the sandbox, and the leaderboard wiring; the contract, the
metric, the baseline, and the scoring path are unchanged. See
[`HARNESS-DESIGN.md`](HARNESS-DESIGN.md) §4–§5 for the anti-cheat analysis and
the precise list of grader-only additions.

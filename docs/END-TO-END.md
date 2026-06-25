# How this repo works, end to end

This is the **map of the whole system**: where the data comes from, how it is
shaped, what the contestant writes, how it is tested, and how a single number
falls out the other end. It is deliberately high-level and cross-links to the
two detailed docs:

- [`WORKFLOW.md`](WORKFLOW.md) — a line-by-line trace of one `cargo run`.
- [`HARNESS-DESIGN.md`](HARNESS-DESIGN.md) — *why* the pieces exist and the
  anti-cheat analysis.

If you only read one section, read [§3 The data transformation chain](#3-the-data-transformation-chain).

---

## 1. The one-sentence version

> Given only the **sparsity pattern** of a sparse symmetric matrix, the
> contestant writes `order()` — a function that returns the elimination order;
> the harness scores that order by the **predicted factorization flops** it
> implies (recomputed by feral's symbolic analysis), as a geometric-mean ratio
> against feral's AMD baseline. **Lower is better; AMD is anchored at 1.00.**

Nothing the contestant returns can make the factorization *wrong* — only
cheaper or more expensive. That property (correctness decoupled from the
objective) is what makes the whole search space safe to expose. See
`README.md` → "Why this matters".

---

## 2. The components

| Component | Path | Role | Trust |
|---|---|---|---|
| **Submission** | `src/ordering/` | the contestant's `order(&Pattern) -> Vec<usize>` — the ONLY editable directory | untrusted |
| **Harness** | `src/main.rs`, `src/corpus.rs`, `src/purity.rs` | runs `order()`, gates it, validates, scores, writes output | trusted |
| **Scoring wrapper** | `ssi-scoring/` | the ONE code path that calls feral: the reader, the scorer, the AMD baseline | trusted |
| **Purity gate** | `ssi-purity/` | the Stage-A scan (shared with the grader) | trusted |
| **Dev corpus** | `corpus/dev/patterns.jsonl` | the input matrices (a small in-repo sample) | public data |
| **Grader** | this same harness, run in `.github/workflows/benchmark.yml` | ranks submissions on a hidden corpus injected via `SSI_CORPUS_FILE` | trusted; the corpus is never published |

The crucial invariant tying these together: **one scoring path**. The harness
(what a contestant runs locally) and the grader (what ranks them) both score by
calling the *same* `ssi-scoring` functions, so a local score predicts the
graded score exactly. See `HARNESS-DESIGN.md` §4.

---

## 3. The data transformation chain

This is the spine of the whole repo — the input matrix's journey from an
optimization problem to a single score:

```
 (UPSTREAM, separate corpus-generation pipeline — not in this repo)
 MINLPLib .nl problem
   → interior-point solve (pounce)            // factor a KKT matrix each iteration
   → capture the KKT sparsity pattern         // structure only, values discarded
   → canonicalize: symmetrize + dedup → CSC, hash
   → stratify + split → dev / eval slices
                         │
                         ▼  one JSON object per line
 ┌─────────────────────────────────────────────────────────────────┐
 │  corpus/dev/patterns.jsonl  (THIS REPO ships a 13-matrix sample)  │
 │  {"n", "nnz", "indptr", "indices", "hash", "source"}              │
 │  — full SYMMETRIZED CSC pattern, INCLUDING the diagonal           │
 └─────────────────────────────────────────────────────────────────┘
                         │
                         ▼  ssi_scoring::pattern_from_jsonl_line   (ssi-scoring/src/loader.rs:78)
                            • parse the 3 structural fields (stdlib JSON)
                            • DROP the diagonal (i == j)
                            • Pattern::from_adjacency: sort + dedup each column
                         ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │  Pattern { n, col_ptr, row_idx }   (ssi-scoring/src/pattern.rs:23)│
 │  — full off-diagonal pattern (both triangles), structure only,    │
 │    no values, no RHS. This is what order() receives.              │
 └─────────────────────────────────────────────────────────────────┘
                         │
            ┌────────────┴─────────────┐
            ▼                          ▼
   order(&Pattern)            amd_baseline(&Pattern)        (ssi-scoring/src/lib.rs:108)
   = contestant perm          = feral_amd::amd_order
            │                          │
            ▼                          ▼
        score(pattern, perm)   ←── SAME function ──→   score(pattern, base_perm)
                         (ssi-scoring/src/lib.rs:86)
                         │
                         ▼  feral's pattern-only building blocks
                            permute_pattern → EliminationTree::from_pattern
                            → column_counts_gnp  (exact column counts c_j of L)
                         ▼
             Score { nnz_l = Σ c_j ,  flops = Σ c_j² }
                         │
                         ▼  per matrix:  ratio = flops(yours) / flops(base)
                            aggregate:   score = geomean(ratio) over the corpus
                         ▼
                  score.json  +  one row in results.tsv
```

### Why each transformation is the way it is

- **Pattern only, no values.** The score depends solely on sparsity structure,
  so the corpus discards numeric values upstream and `Pattern` has no field to
  carry them. The "answer" is physically absent from the contestant's address
  space (NARROW INPUT — `HARNESS-DESIGN.md` §4).
- **The diagonal is dropped at the reader.** The JSONL stores the full matrix
  pattern, which includes the diagonal; the contract `Pattern` is off-diagonal
  (a pure adjacency graph). feral's symbolic algorithms assume the diagonal and
  filter to off-diagonal entries anyway, so dropping it at one place
  (`pattern_from_jsonl_line`) gives a single canonical representation. See
  [`WORKFLOW.md`](WORKFLOW.md) §1.
- **One reader.** `pattern_from_jsonl_line` is the single parse core; the
  harness loads the whole corpus via `load_corpus_jsonl`
  (`ssi-scoring/src/loader.rs:151`) and a future grader can load one line by
  index via `load_pattern_jsonl_line` (`:175`) — both route through the same
  core, so no second parser can silently disagree.
- **`flops = Σ c_j²`, `nnz_l = Σ c_j`.** `c_j` is the exact column count of the
  factor L for column `j` (Gilbert–Ng–Peyton). `Σ c_j` is the fill (nnz of L);
  `Σ c_j²` is a deterministic, hardware-independent proxy for the LDLᵀ
  operation count. See `Score` (`ssi-scoring/src/lib.rs:60`) and
  [`WORKFLOW.md`](WORKFLOW.md) §3.

---

## 4. The run: input → gates → score

Driven by one command:

```sh
cargo run --release -- --note "what I tried"
```

which runs `fn main()` (`src/main.rs`). It executes the production grader's
five stages in miniature; **any failure on any matrix fails the whole run** (no
partial credit):

| Stage | What happens | Code |
|---|---|---|
| **A — purity & license** | scan `src/ordering/` for non-stdlib escapes (added deps, `build.rs`, FFI, `#[no_mangle]`/`#[link]`, proc-macros, `include!` escapes) | `src/purity.rs` → `ssi-purity` |
| **load corpus** | read every line of the corpus file into `(raw_index, name, Pattern)` | `corpus::corpus_indexed()` (`src/corpus.rs`) |
| **B — run order()** | run `order()` **twice** in a killable child process, timed against a 2 s/matrix cap; a panic or cap breach fails the run | `src/main.rs` (`run_once`) |
| **C — validate** | the returned permutation must be a true bijection of `0..n` | `validate_permutation` (`src/main.rs`) |
| **E — determinism** | the two `order()` runs must return byte-identical permutations | `src/main.rs` (`perm1 != perm2`) |
| **D — score** | `score(pattern, perm)` — the same function used for the baseline | `src/main.rs` (`let yours = score`) |
| **aggregate + emit** | `score = exp(mean(ln ratio))`; write `score.json`, append a row to `results.tsv` | `src/main.rs` (`append_results`) |

A full line-by-line trace is in [`WORKFLOW.md`](WORKFLOW.md) §2.

### Outputs

- **`score.json`** — the latest run's score and metrics (machine-readable).
- **`results.tsv`** — append-only history: `timestamp  status  score  fill  note`.

The contestant's code never reports a number — it returns a permutation, and
the harness derives everything. This is what makes the local score equal to the
graded score for the same ordering.

---

## 5. What is tested, and what each test guarantees

`cargo test` runs five independent layers of checks. Together they pin the
claim *"the score is a correct, stable, value-independent function of
(pattern, permutation), and the local path equals the grader path."*

| Test | File | What it guarantees |
|---|---|---|
| **Closed-form scorer facts** | `ssi-scoring/src/lib.rs:136` | The scorer matches hand-derived math: dense 3×3 → flops 14; 5-star hub-first → flops 55; tridiagonal → nnz(L)=2n−1 (zero fill); arrow hub-first → n(n+1)/2; hub-last → near-zero fill. A scorer that fails these is wrong *by definition* (Invariant 4). |
| **Scorer cross-check** | `tests/scorer_crosscheck.rs` | The feral-backed scorer agrees **exactly** (nnz_l and flops) with an *independent* reimplementation (`prototype-oracle`) across arrows, 2D/3D grids, and KKT families, under identity, reverse, and AMD orderings. Two independent codes agreeing rules out a shared bug. |
| **Exact equivalence (pins)** | `tests/exact_equivalence.rs` | Pinned `(nnz_l, flops)` for the identity ordering on three committed sample matrices (`st_e09`, `ex8_5_2`, `gilbert`), so *any* drift in the scoring path breaks the build immediately. `gilbert` (a hub-last arrow) has a closed-form pin (nnz_l = 2·1000+1 = 2001) proving the numbers are genuine, not transcribed. |
| **Narrow input** | `tests/narrow_input.rs` | Two matrices with identical structure but different values load to byte-identical `Pattern`s and score identically — the value column is never consulted. (The JSONL corpus has no values at all; this pins the property for the `.mtx` `load_pattern` path the grader tooling still uses.) |
| **Submission self-checks** | `src/ordering/mod.rs` (`mod tests`) | The shipped starter `order()` returns a valid bijection of `0..n` and handles the empty pattern — the contract the harness enforces. (These are the stub's own tests; you extend them as you build a real ordering.) |

There is **no loader-agreement test** anymore: with a single shared JSONL
reader, the "two parsers might disagree" failure it guarded against cannot
occur (it is prevented by construction, not by assertion). See
`HARNESS-DESIGN.md` §6.

---

## 6. The development loop (for a contestant)

```
read results.tsv + src/ordering/memory/   →   form a hypothesis
        │
        ▼
edit src/ordering/   →   cargo run --release -- --note "hypothesis"
        ▲                        │
        │   per-matrix table + score + any FAIL reason
        └────────────────────────┘
   write findings to src/ordering/memory/ ; commit when the score improves
```

What ships is a **starter stub**: `order()` returns the identity / natural
order — valid and deterministic, but deliberately not a good ordering (it scores
far worse than AMD on real KKT patterns). It is your starting line; replace it
with a real fill-reducing ordering. See [`WORKFLOW.md`](WORKFLOW.md) §4 for the
contract details and a suggested approach.

---

## 7. From local harness to the production grader

**The grader is this same harness binary**, run in the repo's own GitHub Actions
(`.github/workflows/benchmark.yml`) rather than a separate program. The Yukon
platform builds a candidate from the validated baseline + **only** the
submission's `src/ordering/`, dispatches the workflow on it, and reads the
uploaded `score.json`. The workflow grades a **hidden** eval corpus — disjoint
from the dev corpus, drawn from the same distribution, regenerated per round —
injected via the `SSI_CORPUS_FILE` path override, downloaded at run time to a
temp path outside the repo tree (so the eval bytes are never committed). Scoring
runs in a sandbox (no network, no filesystem, a 2–4 GB memory cap, determinism
re-runs).

Because grading runs the identical harness + `ssi-scoring` functions you run
locally, the number a contestant sees locally is structurally the number the
grader reproduces for the same ordering. What grading adds is the hidden corpus,
the sandbox, and the platform wiring (PR per submission, dispatch, score
comment, accept/close); the contract, the metric, the baseline, and the scoring
path are unchanged. The anti-cheat reasoning is in `HARNESS-DESIGN.md` §4–§5.

### The corpus, sample vs. full

What ships in this repo is a **13-matrix sample** (spanning families
NLP / QCP / QP / QCQP plus one mid-size sparse case) so the pipeline runs
immediately after a clone — it is **not** a representative tuning set. The full
development corpus (~279 patterns, n up to ~340k) is published for download
separately; the hidden evaluation corpus is never published. See
[`corpus/dev/README.md`](../corpus/dev/README.md).

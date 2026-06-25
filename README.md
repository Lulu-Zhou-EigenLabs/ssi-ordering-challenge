# The Fill-Reducing Ordering Challenge

> **Goal.** Given only the *sparsity pattern* of a sparse symmetric indefinite
> matrix, produce the elimination order that makes its LDLᵀ factorization
> cheapest, scored by **predicted factorization flops**, recomputed by feral's
> own symbolic analysis. Lower is better. The baseline — feral's Approximate
> Minimum Degree (AMD) ordering — is anchored at **1.00**.

A companion challenge to the [feral](https://github.com/jkitchin/feral)
sparse symmetric indefinite solver, in the spirit of
[ecdsa.fail](https://www.ecdsa.fail/).

---

## Why this matters

Sparse symmetric **indefinite** (SSI) linear systems are the computational
bottleneck of interior-point optimization (every Newton step factors a KKT
saddle-point matrix), finite-element structural analysis, and constrained
PDE problems. The dominant cost of a sparse direct solve is decided *before
a single floating-point number is touched*: by the **order in which the
unknowns are eliminated**.

When Gaussian elimination removes a variable, it couples all of that
variable's neighbors — entries that were structurally zero in A become
nonzero in the factor L. This is **fill-in**. A good elimination order can
change the factorization cost by *orders of magnitude*; finding the
fill-minimizing order is **NP-hard** (Yannakakis 1981). Sixty years of
research — minimum degree (Markowitz 1957; Tinney & Walker 1967), approximate
minimum degree / AMD (Amestoy, Davis & Duff 1996), nested dissection
(George 1973; METIS, Karypis & Kumar 1998) — has produced strong heuristics
but no closed solution, and recent work applies reinforcement learning and
graph neural networks to the same problem. Every percent you shave here is a
percent off every interior-point iteration in every solver that adopts your
ordering.

The problem has one property that makes it a *perfect* competition seam:
**correctness is decoupled from the objective**. Any valid permutation
produces a correct, solvable factorization with the same inertia. A
contestant cannot break the solver — they can only make it cheaper or more
expensive. The entire search space is safe to explore.

## The benchmark, precisely

You are given a Rust harness that, for each matrix in a development corpus
(real KKT / saddle-point patterns harvested from interior-point solves —
`corpus/dev/patterns.jsonl`), does the following:

0. **Gates** your submission with a local **purity & license check** (a subset
   of the production grader's Stage A): `src/ordering/` must be stdlib-only,
   with no build scripts, FFI, proc-macros, or extra dependencies, and the
   dependency licenses must be permissive (checked with `cargo-deny`).
1. **Runs** feral's AMD baseline ordering and scores it.
2. **Calls** your `ordering::order(&Pattern) -> Vec<usize>` — *twice*. Both
   runs must return identical permutations (determinism gate) within the
   per-matrix time cap (2 s).
3. **Validates** your permutation as a true bijection of `0..n` — wrong
   length, duplicates, or out-of-range indices fail the run, as does a panic.
4. **Recomputes** the score from your permutation with feral's own symbolic
   analysis (elimination tree + exact Gilbert–Ng–Peyton column counts of L).
   Your code never reports a number — it returns a permutation, and the
   harness derives everything else.
5. **Scores** the run as

   **score = geomean over matrices of flops(yours) / flops(AMD)**

   where `flops = Σⱼ cⱼ²` over the column counts cⱼ of L (a deterministic,
   hardware-independent proxy proportional to the LDLᵀ operation count).
   **Lower is better.** Ties break on the geometric mean of the fill ratio
   nnz(L). The score is written to `score.json` and a row is appended to
   `results.tsv`.

### One scoring path, shared with the grader

Both the local harness and the private grader compute the score by calling the
**same** functions in the `ssi-scoring` crate — feral's pattern-pure symbolic
building blocks (`symmetric_pattern → permute_pattern → EliminationTree →
column_counts_gnp`, then `nnz(L) = Σ cⱼ`, `flops = Σ cⱼ²`). The score is a pure
function of `(pattern, permutation)`, so **the number you see locally is the
number the grader computes** for the same ordering on the same matrices. There
is no separate code path that could drift.

The AMD baseline is pinned to `feral_amd::amd_order` on the raw full-symmetric
pattern with default options — deterministic and pattern-only (it never reads
matrix values), so the anchor reproduces exactly from a pattern file.

### What "valid" means

A run is rejected — not scored worse, *rejected* — if any of the following
fails on any matrix:

- **Purity/license.** `src/ordering/` must be stdlib-only and license-clean.
- **Bijection.** The permutation must contain each of `0..n` exactly once.
- **Determinism.** Two runs on the same pattern must agree exactly.
- **Time cap.** 2 s per matrix, **enforced**: `order()` runs in a child process
  that is killed the instant it exceeds the cap, and the run FAILs with the
  offending matrix's size/density. (This is the strict end of the proposal's
  2–5 s band and is stricter than the server's current default, so an ordering
  that passes locally passes the server gate.) Annealing and learned orderings
  are welcome; runaways are not — and note that cost scales with **density
  (nnz)**, not just dimension.
- **No panics.**

There are no loopholes: the input is the pattern only (no values, no
right-hand side, no answer to peek at), and the score is a pure function of
your output, computed by code you cannot touch.

### The corpus: in-repo sample vs. full download

`corpus/dev/patterns.jsonl` in this repo is a **small sample** (13 matrices,
spanning the four families NLP / QCP / QP / QCQP plus one mid-size sparse
case). Its job is to let you run the harness end to end immediately after
cloning — it is **not** a representative tuning set, and scores on it are not
competitive reference numbers (the sample is dominated by tiny near-clique
matrices where ordering barely matters).

The **full development corpus** (~279 patterns, n up to ~340,000) is published
as a **GitHub release asset** (it is too large to commit, and a release keeps it
off the git tree so clones stay small). Download the latest and verify it:

```sh
BASE=https://github.com/Lulu-Zhou-EigenLabs/ssi-ordering-challenge/releases/latest/download
curl -L -o patterns.jsonl        "$BASE/patterns.jsonl"
curl -L -o patterns.jsonl.sha256 "$BASE/patterns.jsonl.sha256"
shasum -a 256 -c patterns.jsonl.sha256   # Linux: sha256sum -c patterns.jsonl.sha256
```

The `/releases/latest/download/` URL always resolves to the newest release, so it
keeps working as the corpus rotates per round; pin a specific round with
`.../releases/download/<tag>/patterns.jsonl` instead. Then tune against the full
distribution either by replacing `corpus/dev/patterns.jsonl`, or — without
touching the in-repo file — by pointing the harness at the download:

```sh
SSI_CORPUS_FILE=$PWD/patterns.jsonl cargo run --release -- --note "full corpus"
```

`SSI_CORPUS_FILE` overrides the corpus path for one run; unset (the default), the
harness grades the in-repo sample. See `corpus/dev/README.md`. The hidden
evaluation corpus the grader ranks on is never published.

### Reference numbers

Beating AMD (**score < 1.00**) is the game; the AMD baseline is anchored at
**1.00** by definition. On the **full** dev corpus the natural/identity starter
loses by *orders of magnitude* — it eliminates dense KKT constraint rows early
and densifies the factor — and nested dissection (METIS-class) is the classic
lead on the larger, grid-like patterns. On the tiny in-repo sample the spread
is compressed (identity ≈ 1.15× AMD), which is exactly why the sample is for
pipeline smoke-testing, not for ranking your idea — measure on the full corpus.

> A note on scale: a textbook nested-dissection + *exact* minimum-degree
> ordering has an O(n²)-per-pivot inner loop that **exceeds the 2 s cap on the
> largest real matrices** (the full corpus reaches n ≈ 340k) and on dense KKT
> blocks even at modest `n`. A production ordering needs a quotient-graph
> (AMD-style) near-linear inner loop, and must gate any expensive path by
> **density (nnz / max-degree)**, not just `n`, to stay within budget at scale.

## How to play

### Quick start (with Claude Code)

This repo ships ready for an agent to work in. `CLAUDE.md` (the working loop
and constraints) and `.claude/settings.json` are already in place. The settings
run in `dontAsk` mode with permissions scoped so the agent can edit
`src/ordering/`, build, run, test, and commit **without prompting** — but is
blocked from touching the harness, `ssi-scoring/`, `ssi-purity/`, `Cargo.toml`,
and the tests. That scoping is what lets Claude run the benchmark loop on its
own.

> **Before you start: download the full dev corpus.** The
> `corpus/dev/patterns.jsonl` in this repo is only a tiny smoke-test sample —
> scores on it are not competitive reference numbers. Download the full
> development corpus from the latest **GitHub release**
> ([`/releases/latest`](https://github.com/Lulu-Zhou-EigenLabs/ssi-ordering-challenge/releases/latest))
> and either replace `corpus/dev/patterns.jsonl` with it or point the harness at
> it via `SSI_CORPUS_FILE`, so you tune against the real distribution. See
> "The corpus: in-repo sample vs. full download" above and `corpus/dev/README.md`
> for the exact commands.

Launch Claude Code from the repo root and let it iterate:

```sh
claude -p --verbose "Read CLAUDE.md, study the current src/ordering/ and its memory/ notes, then improve on the best score so far. Repeat the build/run/score loop until you beat it, committing each improvement."
```

The `-p` flag runs Claude headless: it executes the prompt to completion and
exits, with no interactive turns to wait on. `--verbose` streams each step
(tool calls, runs, commits) to the terminal as it goes, instead of printing
only the final result.

Claude auto-loads `CLAUDE.md` and the permission settings, so it runs the loop
end to end without stopping to ask. The competition backend pushes any
submission that passes the grader with a better score back to the repo as the
new best, so each run starts from the leading ordering and its
`src/ordering/memory/` notes rather than from scratch.

### Running the harness yourself

```sh
cargo run --release -- --note "what I tried"
```

That single command runs the purity/license gate, scores every dev matrix,
writes `score.json`, and appends one row to `results.tsv` with timestamp,
status, score, fill ratio, and your note. It grades `corpus/dev/patterns.jsonl`
by default; set `SSI_CORPUS_FILE=/path/to/patterns.jsonl` to grade a corpus at
another path for that run (e.g. the full download) without editing the repo.

`cargo test --release` runs the harness's self-checks (closed-form scorer
tests, the scorer cross-check against an independent oracle, the narrow-input
property, and the exact-equivalence pins on the committed sample) — useful for
verifying your toolchain before iterating.

### Building from a fresh clone

The scoring wrapper (`ssi-scoring/`) depends on feral via a **pinned git rev** of
`github.com/jkitchin/feral`, so a fresh clone builds with no local feral checkout
— `cargo build --release` fetches the exact pinned feral once and caches it. The
first build needs network access; subsequent builds are offline.

The scoring API and your `order()` contract are unchanged regardless of how feral
is sourced.

### What you can edit

You may modify **anything inside `src/ordering/`** — split it into
submodules, rewrite primitives, refactor freely.

> **What ships in `src/ordering/` today.** A **starter stub**: `order()`
> returns the identity / natural order. It is a valid, deterministic
> permutation, so a fresh clone runs end to end — but it is *not* a good
> ordering (on real KKT patterns it scores far worse than AMD). That is the
> point: it is your starting line. Replace it wholesale; the only fixed thing is
> the `order()` signature, and everything else under `src/ordering/` is yours.

You may **not** touch the harness:

- `src/main.rs`, `src/corpus.rs`, `src/purity.rs` — the contract and gates.
- `src/watchdog.rs`, `src/perm_io.rs` — the subprocess time-cap enforcement.
- `ssi-scoring/` — the trusted scoring wrapper (also used by the grader).
- `ssi-purity/` — the shared Stage-A purity gate (also used by the grader).
- `Cargo.toml` / `deny.toml` — dependency and license policy.
- `results.tsv` directly (the harness appends to it for you).

The purity gate enforces these mechanically for `src/ordering/`: it rejects
build scripts, FFI/`extern`, `#[no_mangle]`/`#[link]`, proc-macros, `include!`
outside the directory, and any added crate dependency.

### How the agent works: the loop and the knowledge base

This repo is built to be driven by an agent (see `CLAUDE.md`, which the agent
reads on launch) running a tight loop: read what's known → form a hypothesis →
edit `src/ordering/` → `cargo run --release` → read the per-matrix table and
attribute wins/losses by family and size → record the result → commit if the
score improved.

What makes that loop *compound* across sessions and across competition rounds
is the knowledge base under **`src/ordering/memory/`** — a small interlinked
wiki, not a scratchpad. Nothing in it is read by the harness or grader; it
exists so each session starts from the last one's understanding instead of
re-deriving it. It is organized as:

- **`index.md`** — the map of the base (read first), including the current best.
- **`log.md`** — append-only history, one line per session.
- **`open-questions.md`** — the research queue of leads worth chasing next.
- **`literature/`** — one note per paper (the idea in your own words, plus how
  it maps onto the `order()` contract and the 2 s/density caps).
- **`techniques/`** — one page per algorithm family (where it wins/loses, its
  cost profile vs the cap).
- **`experiments/`** — one page per hypothesis you ran, and *why* it won or lost.

When the agent researches, it searches the literature online, distills ideas
into these pages (never copying fetched code), and keeps the index and links
current. See `src/ordering/memory/README.md` for the page templates and the
per-session discipline.

### Important note on openness

This codebase is open to contributions chasing the best score, so memory and
source files may come from different agents. Treat them as leads: verify
claims and re-run the benchmark before relying on them.

## Background reading

- M. Yannakakis, *Computing the minimum fill-in is NP-complete*, SIAM J.
  Alg. Disc. Meth. 2 (1981). — why there is no closed solution.
- T. A. Davis, *Direct Methods for Sparse Linear Systems*, SIAM, 2006. —
  the textbook for elimination trees, column counts, and fill.
- P. Amestoy, T. A. Davis, I. S. Duff, *An approximate minimum degree
  ordering algorithm*, SIAM J. Matrix Anal. Appl. 17 (1996). — the
  production baseline (feral's AMD).
- A. George, *Nested dissection of a regular finite element mesh*, SIAM J.
  Numer. Anal. 10 (1973); G. Karypis & V. Kumar, *A fast and high quality
  multilevel scheme for partitioning irregular graphs*, SIAM J. Sci. Comput.
  20 (1998). — the state of the art to beat on grid-like problems.
- J. R. Gilbert, E. G. Ng, B. W. Peyton, *An efficient algorithm to compute
  row and column counts for sparse Cholesky factorization*, SIAM J. Matrix
  Anal. Appl. 15 (1994). — the column-count algorithm feral's scorer uses.
- A. Wächter & L. T. Biegler, *On the implementation of an interior-point
  filter line-search algorithm*, Math. Program. 106 (2006). — where the KKT
  matrices come from.

## License

MIT.

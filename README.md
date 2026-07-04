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

0. **Gates** your submission with a local **purity & license check** (the same
   Stage A the production grader runs): `src/ordering/` must be pure Rust — no
   build scripts, FFI, `#[no_mangle]`/`#[link]`, proc-macros, or `include!`
   outside the directory. You MAY depend on permissive, pure-Rust crates by
   declaring them in `src/ordering/deps.toml` (see "Using crates" below); every
   dependency and its whole transitive tree must be permissively licensed
   (checked with `cargo-deny`, which must be installed) and contain no native
   code (`*-sys`, a `links`/`cc` build that compiles C, or a shipped blob are
   rejected).
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

   **score = weighted mean over size buckets of the within-bucket geomean of
   flops(yours) / flops(AMD)**

   Matrices are bucketed by dimension n — **lt_1k** (n<1000), **1k_10k**
   (1000≤n<10000), **gt_10k** (n≥10000) — with weights **0.30 / 0.30 / 0.40**.
   The larger matrices carry the most weight because that is where real-world
   value and algorithmic difficulty concentrate, and because a size-biased corpus
   would otherwise let the small-matrix tail dominate. Within each bucket the
   score is the geomean of `flops = Σⱼ cⱼ²` ratios (a deterministic,
   hardware-independent proxy). Empty buckets are renormalized out. **Lower is
   better.** Ties break on the same weighted scheme over the fill ratio nnz(L).
   The score (and per-bucket detail) is written to `score.json`; a row is
   appended to `results.tsv`.

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

- **Purity/license.** `src/ordering/` must be pure Rust (no FFI/build scripts);
  any crates declared in `deps.toml` and their whole tree must be pure Rust and
  permissively licensed.
- **Bijection.** The permutation must contain each of `0..n` exactly once.
- **Determinism.** Two runs on the same pattern must agree exactly.
- **Time cap.** 2 s per matrix, **enforced**: `order()` runs in a child process
  that is killed the instant it exceeds the cap, and the run FAILs with the
  offending matrix's size/density. (This is the strict end of the proposal's
  2–5 s band. The grader runs this same harness, so the cap that gates your
  submission on the server is exactly the cap you see locally.) Annealing and
  learned orderings are welcome; runaways are not — and note that cost scales
  with **density (nnz)**, not just dimension.
- **No panics.**

There are no loopholes: the input is the pattern only (no values, no
right-hand side, no answer to peek at), and the score is a pure function of
your output, computed by code you cannot touch.

### The corpus

`corpus/dev/patterns.jsonl` is the **full development corpus** (300 patterns,
n up to ~340,000, spanning the families NLP / QCP / QP / QCQP). It is shipped
in-repo via **Git LFS** (the file is ~99 MB), so after cloning it is simply
present — no separate download step.

**Install Git LFS before you clone** (or fetch after):

```sh
git lfs install
# if you already cloned without git-lfs installed:
git lfs pull
```

Without `git-lfs`, the working-tree file is a small text *pointer*, not JSONL,
and the harness will stop with a message telling you to run `git lfs pull`.

You can point the harness at a different corpus for one run with the
`SSI_CORPUS_FILE` override (an absolute path outside the repo tree):

```sh
SSI_CORPUS_FILE=/path/to/other.jsonl cargo run --release --offline --locked -- --note "other corpus"
```

Unset (the default), the harness grades the in-repo corpus. The competition's
hidden evaluation corpus is never published.

### Reference numbers

Beating AMD (**score < 1.00**) is the game; the AMD baseline is anchored at
**1.00** by definition. The shipped starter in `src/ordering/` calls
`feral_amd::amd_order` — the same crate/version as the baseline — so it ties
the baseline at **1.00**. It is a minimal, correct starting point to improve on:
nested dissection (METIS-class) is the classic lead on larger, grid-like
patterns. The natural/identity ordering (not shipped) would lose by orders of
magnitude — it eliminates dense KKT constraint rows early and densifies the
factor.

> A note on scale: a textbook nested-dissection + *exact* minimum-degree
> ordering has an O(n²)-per-pivot inner loop that **exceeds the 2 s cap on the
> largest real matrices** (the full corpus reaches n ≈ 340k) and on dense KKT
> blocks even at modest `n`. A production ordering needs a quotient-graph
> (AMD-style) near-linear inner loop, and must gate any expensive path by
> **density (nnz / max-degree)**, not just `n`, to stay within budget at scale.

## How to play

### Quick start (with an agent)

The rules for working in this repo live in [`RULES.md`](RULES.md) — the working
loop, the constraints, and the knowledge-base discipline, written to be followed
by hand or by any coding agent. It does not assume a particular editor or tool.

If you drive the repo with a coding agent, point it at that file (e.g. "read and
follow `RULES.md`"). The rules say to edit only `src/ordering/`, build, run,
test, and commit — and to leave the harness, `ssi-scoring/`, `ssi-purity/`,
`Cargo.toml`, and the tests alone (the grader rebuilds those from its own copy
regardless). If your tool supports scoped permissions or an allowlist, scoping
edits to `src/ordering/` plus `cargo`/`git` is a convenient way to let it run
the loop unattended.

> **Before you start: make sure Git LFS is installed** (`git lfs install`), so
> `corpus/dev/patterns.jsonl` resolves to the real 300-pattern corpus rather
> than an LFS pointer. If you cloned without it, run `git lfs pull`. See
> "The corpus" above and `corpus/dev/README.md`.

A headless run from the repo root looks like this (Claude Code shown; adapt the
command to your agent):

```sh
claude -p --verbose "Read RULES.md, study the current src/ordering/ and its memory/ notes, then improve on the best score so far. Repeat the build/run/score loop until you beat it, committing each improvement."
```

The `-p` flag runs the agent headless: it executes the prompt to completion and
exits, with no interactive turns to wait on. `--verbose` streams each step (tool
calls, runs, commits) to the terminal as it goes, instead of printing only the
final result.

The competition backend pushes any submission that passes the grader with a
better score back to the repo as the new best, so each run starts from the
leading ordering and its `src/ordering/memory/` notes rather than from scratch.

### Running the harness yourself

```sh
bash scripts/prepare-build.sh            # regenerate Cargo.toml from deps.toml + vendor + scan
cargo run --release --offline --locked -- --note "what I tried"
```

`prepare-build.sh` validates `src/ordering/deps.toml`, regenerates the trusted
`Cargo.toml` from `Cargo.toml.in` + your declared crates, vendors the full
dependency tree, and scans it for native-code signals (`*-sys` names, prebuilt
blobs, C-toolchain build-deps). Run it whenever you change `deps.toml` (and once
on a fresh clone). If your `deps.toml` is empty you can skip it and just
`cargo run --release`.

The scored command runs the purity/license gate, scores every dev matrix,
writes `score.json`, and appends one row to `results.tsv` with timestamp,
status, score, fill ratio, and your note. It grades `corpus/dev/patterns.jsonl`
by default; set `SSI_CORPUS_FILE=/path/to/patterns.jsonl` to grade a corpus at
another path for that run (e.g. the full download) without editing the repo.

`cargo test --release` runs the harness's self-checks (closed-form scorer
tests, the scorer cross-check against an independent oracle, the narrow-input
property, and the exact-equivalence pins on the committed sample) — useful for
verifying your toolchain before iterating.

> **`cargo-deny` is required.** The gate runs the authoritative `cargo-deny`
> license check (RequireDeny mode) — install it once with
> `cargo install cargo-deny`. A missing `cargo-deny` fails the gate rather than
> skipping it, so your local result matches the grader.

### Using crates

You may depend on permissive, **pure-Rust** crates. Declare them in
`src/ordering/deps.toml` — a restricted TOML file with a single `[dependencies]`
table of exact-version entries:

```toml
[dependencies]
rand = "0.8.5"
petgraph = "0.6.4"
```

Only `name = "x.y.z"` is accepted: no `git`/`path`/`features` keys, no version
ranges or `*`, no other sections. Every declared crate **and its whole
transitive tree** must come from crates.io, be permissively licensed (MIT /
Apache-2.0 / BSD / Unlicense / Zlib / Unicode-3.0), and be pure Rust — a `*-sys`
wrapper, a crate that compiles C (a `cc`/`cmake`/`bindgen` build-dependency or a
`links` native library), or one shipping a prebuilt binary blob is rejected.
After editing `deps.toml`, run `bash scripts/prepare-build.sh` before `cargo
run`. See [`docs/DECISION-crate-policy.md`](docs/DECISION-crate-policy.md) for
the full policy and its rationale.

### Building from a fresh clone

The scoring wrapper (`ssi-scoring/`) depends on feral via **exact crates.io
releases** (`feral = "=0.11.0"` and companions), so a fresh clone builds with no
local feral checkout. `prepare-build.sh` fetches and vendors everything once
(needs network); the subsequent `cargo build/run --offline --locked` needs no
network. The scoring API and your `order()` contract are unchanged.

> **Generated `Cargo.toml`.** The committed source of truth is `Cargo.toml.in`;
> `prepare-build.sh` generates `Cargo.toml` (git-ignored) from it plus your
> `deps.toml`. Don't edit `Cargo.toml` by hand — edit `deps.toml` and re-run the
> script.

### What you can edit

You may modify **anything inside `src/ordering/`** — split it into
submodules, rewrite primitives, refactor freely.

> **What ships in `src/ordering/` today.** A **starter stub**: `order()`
> returns the identity / natural order. It is a valid, deterministic
> permutation, so a fresh clone runs end to end — but it is *not* a good
> ordering (on real KKT patterns it scores far worse than AMD). That is the
> point: it is your starting line. Replace it wholesale; the only fixed thing is
> the `order()` signature, and everything else under `src/ordering/` is yours.

You may also declare crates in **`src/ordering/deps.toml`** (see "Using crates").
That is the one file outside your algorithm code you may edit — the grader reads
only `src/ordering/` (including its `deps.toml`) from your fork.

You may **not** touch the harness:

- `src/main.rs`, `src/corpus.rs`, `src/purity.rs` — the contract and gates.
- `src/watchdog.rs`, `src/perm_io.rs` — the subprocess time-cap enforcement.
- `ssi-scoring/` — the trusted scoring wrapper (also used by the grader).
- `ssi-purity/` — the shared Stage-A purity gate (also used by the grader).
- `Cargo.toml.in` / `deny.toml` — the trusted manifest template and license
  policy. (`Cargo.toml` is generated from `Cargo.toml.in` + your `deps.toml`;
  don't edit it by hand.)
- `results.tsv` directly (the harness appends to it for you).

The purity gate enforces this mechanically for `src/ordering/`: it rejects
build scripts, FFI/`extern`, `#[no_mangle]`/`#[link]`, proc-macros, and
`include!` outside the directory. Declared dependencies are additionally scanned
across their whole transitive tree for native code and non-permissive licenses.

### How the agent works: the loop and the knowledge base

This repo is built to be driven by an agent (see [`RULES.md`](RULES.md), which
describes the loop) running a tight cycle: read what's known → form a hypothesis →
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

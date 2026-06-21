# JSONL Corpus Reader (Local Harness) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the local harness read the new `corpus/dev/patterns.jsonl` (CSC sparsity patterns, one JSON object per line) produced by the `corpus-generation` pipeline, replacing the old per-file `.mtx` dev corpus — via a single shared reader in the trusted `ssi-scoring` crate.

**Architecture:** Add one pure parse function and two thin I/O wrappers to `ssi-scoring`. The pure core (`pattern_from_jsonl_line`) is the single definition of "JSONL line → `Pattern`", deliberately split from I/O so the grader can later reuse the core while addressing matrices by `(path, line_index)` for its process-per-matrix sandbox. The harness's `dev_corpus()` switches from walking `*.mtx` to streaming the JSONL through the shared core. The frozen contract (`Pattern`, `order()`, score definition, `score.json`/`results.tsv` formats) does not change. This plan covers the **local harness only**; the grader is out of scope but the reader API is shaped so the grader can build on it.

**Tech Stack:** Rust (stable, edition 2021), stdlib-only JSON parsing in `ssi-scoring` (no new crate dependency), `cargo test`.

## Global Constraints

- **THE CONTRACT IS FROZEN (Invariant 1):** do not change the `order(pattern: &Pattern) -> Vec<usize>` signature, the score definition (geomean of flops(yours)/flops(AMD)), the validity gates, or the `score.json` / `results.tsv` output formats. Score *values* may shift with the new corpus — re-measure, don't redefine.
- **ONE SCORING / ONE READER PATH (Invariant 2):** harness and grader must parse a corpus line into a `Pattern` through the *same* function. After this plan there is exactly one parser; the old dual-reader cross-check (`tests/loader_agreement.rs`) is deleted because its failure mode (two parsers diverging) can no longer occur.
- **SUBMISSION DIRECTORY STAYS STDLIB-ONLY (Invariant 3):** do not add any dependency to `src/ordering/`. The new reader lives in `ssi-scoring` (trusted), which the harness already depends on; this does not widen the contestant boundary. Do **not** add a JSON crate to `ssi-scoring` — use a focused stdlib parser.
- **CLOSED-FORM TESTS ALWAYS PASS (Invariant 4):** the scorer tests in `ssi-scoring/src/lib.rs` (`dense_3x3_flops_14`, `star_5_hub_first_flops_55`, `tridiagonal_zero_fill`, arrow tests) are reader-independent and must remain green untouched.
- **GREEN AND COMMITTED (Invariant 5):** `cargo test` passes at the end of every task; commit at each working milestone.
- **READ, DON'T GUESS (Invariant 6):** the JSONL schema is pinned by `corpus-generation/docs/SCHEMA.md` and `README.md`: each line is `{"n", "nnz", "indptr", "indices", "hash", "source"}`, a **full symmetrized** CSC pattern that **includes the diagonal** (`indices` for column `j` contains `j`). The contract `Pattern` is off-diagonal; the reader MUST drop `i == j`.

### Corpus format reference (verbatim from the pipeline)

One line of `patterns.jsonl`:

```json
{"n":4,"nnz":12,"indptr":[0,3,6,8,12],"indices":[0,1,3,0,1,3,2,3,0,1,2,3],"hash":"9cce0c0e...","source":"st_e09"}
```

- `n`: matrix dimension.
- `indptr`: CSC column pointers, length `n+1`, `indptr[0]==0`, non-decreasing.
- `indices`: CSC row indices, length `indptr[n]` (== `nnz`); column `j` owns `indices[indptr[j]..indptr[j+1]]`. **Includes the diagonal entry `j`.**
- `hash`, `source`, `nnz`: metadata. `source` is the MINLPLib problem name; the harness uses it as the display name.

The current new dev corpus: **279 patterns**, smallest `n=4`, largest `n=339994`.

---

## File Structure

- `ssi-scoring/src/loader.rs` — **gains** the JSONL reader (pure core + two I/O wrappers). **Keeps** `load_pattern` (feral `.mtx` reader) for now; the grader still uses it, and removing it is out of scope. Module doc updated.
- `ssi-scoring/src/lib.rs` — re-export the new public functions alongside `load_pattern`.
- `ssi-ordering-challenge/src/pattern.rs` — `dev_corpus()` rewritten to stream `corpus/dev/patterns.jsonl` via the shared reader. The stdlib `.mtx` reader (`read_mtx_pattern` / `parse_mtx_pattern` / `pattern_from_adjacency`) is **deleted** — nothing else uses it once the cross-check test is gone. `DEV_CORPUS_DIR` becomes a file path constant.
- `ssi-ordering-challenge/src/main.rs` — only the error message referencing `DEV_CORPUS_DIR` may need a wording tweak; the scoring loop is untouched.
- `tests/loader_agreement.rs` — **deleted** (its reason to exist — two parsers — is gone).
- `corpus/dev/` — old `.mtx` tree **removed**; `patterns.jsonl` **added**.
- `README.md`, `docs/HARNESS-DESIGN.md` — reference scores, family taxonomy (NLP/QCP/QP/QCQP), and corpus-format prose updated.

**Out of scope (noted for the grader follow-up, do NOT implement here):** `grader/runner/src/{score.rs,worker.rs,watchdog.rs}` switching to `(jsonl_path, line_index)`; the empty eval slice (`eval_size: 0`) needing a real eval-generation run; Git-LFS / size policy for the ~225 MB `patterns.jsonl`.

---

## Task 1: Pure JSONL-line → Pattern core in ssi-scoring

**Files:**
- Modify: `ssi-scoring/src/loader.rs` (add parse core + error variant)
- Modify: `ssi-scoring/src/lib.rs:52` (re-export)
- Test: `ssi-scoring/src/loader.rs` (`#[cfg(test)] mod tests`, append)

**Interfaces:**
- Consumes: `crate::pattern::Pattern`, `Pattern::from_adjacency(n, &mut [Vec<usize>])` (already `pub(crate)` in `ssi-scoring/src/pattern.rs:47`).
- Produces:
  - `pub fn pattern_from_jsonl_line(line: &str) -> Result<(String, Pattern), LoadError>` — returns `(source, pattern)`. Drops the diagonal; symmetry/sort/dedup handled by `from_adjacency`.
  - New `LoadError::Json(String)` variant.

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block in `ssi-scoring/src/loader.rs`:

```rust
#[test]
fn jsonl_line_parses_and_drops_diagonal() {
    // n=4 line from the real corpus (source st_e09). indices include the
    // diagonal (each column j lists j); the contract Pattern must drop it.
    let line = r#"{"n":4,"nnz":12,"indptr":[0,3,6,8,12],"indices":[0,1,3,0,1,3,2,3,0,1,2,3],"hash":"9cce0c0e","source":"st_e09"}"#;
    let (source, p) = pattern_from_jsonl_line(line).unwrap();
    assert_eq!(source, "st_e09");
    assert_eq!(p.n, 4);
    // Column 0 raw = {0,1,3}; diagonal 0 dropped -> {1,3}.
    assert_eq!(p.col(0), &[1, 3]);
    // Column 2 raw = {2,3}; diagonal 2 dropped -> {3}.
    assert_eq!(p.col(2), &[3]);
    // Column 3 raw = {0,1,2,3}; diagonal 3 dropped -> {0,1,2}.
    assert_eq!(p.col(3), &[0, 1, 2]);
    // 12 raw entries - 4 diagonal = 8 off-diagonal.
    assert_eq!(p.nnz(), 8);
}

#[test]
fn jsonl_line_rejects_malformed() {
    assert!(pattern_from_jsonl_line("not json").is_err());
    assert!(pattern_from_jsonl_line(r#"{"n":2}"#).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ssi-scoring jsonl_line -- --nocapture`
Expected: FAIL — `cannot find function pattern_from_jsonl_line`.

- [ ] **Step 3: Add the `Json` error variant**

In `ssi-scoring/src/loader.rs`, extend `LoadError`:

```rust
/// Failure to load a `.mtx` file or a JSONL corpus line into a `Pattern`.
#[derive(Debug)]
pub enum LoadError {
    /// feral's reader rejected the file (bad banner, missing size line, …).
    Read(String),
    /// A `patterns.jsonl` line was malformed or structurally invalid.
    Json(String),
}
```

And extend the `Display` impl's match:

```rust
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(m) => write!(f, "failed to read .mtx: {m}"),
            LoadError::Json(m) => write!(f, "failed to parse JSONL pattern: {m}"),
        }
    }
}
```

- [ ] **Step 4: Implement the pure parse core**

Add to `ssi-scoring/src/loader.rs` (after `from_feral_symmetric`). A focused stdlib parser — the pipeline emits deterministic, compact JSON, so we extract exactly the three structural fields plus `source` rather than pull in a JSON crate (keeps `ssi-scoring` dependency-light, Invariant 3 spirit):

```rust
/// Parse ONE line of `patterns.jsonl` into `(source, Pattern)`.
///
/// The pipeline (`corpus-generation`) emits a full *symmetrized* CSC pattern
/// that INCLUDES the diagonal (column `j`'s indices contain `j`). The contract
/// `Pattern` is off-diagonal, both-triangle, so we drop `i == j` and rebuild
/// via `Pattern::from_adjacency` (which sorts, dedups, and validates symmetry).
///
/// This is the SINGLE definition of "corpus line -> Pattern". The harness loads
/// the whole corpus through it; a future grader can address one line by index
/// and route it through this same core (Invariant 2 at the parsing boundary).
pub fn pattern_from_jsonl_line(line: &str) -> Result<(String, Pattern), LoadError> {
    let err = |m: &str| LoadError::Json(format!("{m} in line: {}", truncate(line, 80)));

    let n = parse_usize_field(line, "\"n\"").ok_or_else(|| err("missing/invalid \"n\""))?;
    let indptr = parse_int_array(line, "\"indptr\"").ok_or_else(|| err("missing \"indptr\""))?;
    let indices = parse_int_array(line, "\"indices\"").ok_or_else(|| err("missing \"indices\""))?;
    let source = parse_string_field(line, "\"source\"").unwrap_or_default();

    if indptr.len() != n + 1 {
        return Err(err("indptr length != n+1"));
    }
    if indptr.first() != Some(&0) {
        return Err(err("indptr[0] != 0"));
    }
    if indptr.last().copied() != Some(indices.len()) {
        return Err(err("indptr[n] != indices.len()"));
    }

    // Expand CSC -> per-column adjacency, dropping the diagonal.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for j in 0..n {
        let (lo, hi) = (indptr[j], indptr[j + 1]);
        if lo > hi || hi > indices.len() {
            return Err(err("non-monotone indptr"));
        }
        for &i in &indices[lo..hi] {
            if i >= n {
                return Err(err("row index out of range"));
            }
            if i != j {
                adj[j].push(i);
            }
        }
    }
    Ok((source, Pattern::from_adjacency(n, &mut adj)))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max]) }
}

/// Find `"key":<digits>` and parse the integer. Returns None if absent/invalid.
fn parse_usize_field(line: &str, key: &str) -> Option<usize> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start_matches([':', ' ']);
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Find `"key":"value"` and return the string value (no escapes expected).
fn parse_string_field(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start_matches([':', ' ']);
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Find `"key":[<comma-separated ints>]` and parse into a Vec<usize>.
fn parse_int_array(line: &str, key: &str) -> Option<Vec<usize>> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start_matches([':', ' ']);
    let rest = rest.strip_prefix('[')?;
    let end = rest.find(']')?;
    let body = &rest[..end];
    if body.trim().is_empty() {
        return Some(Vec::new());
    }
    body.split(',').map(|t| t.trim().parse::<usize>().ok()).collect()
}
```

- [ ] **Step 5: Re-export from lib.rs**

In `ssi-scoring/src/lib.rs:52`, change:

```rust
pub use loader::{load_pattern, LoadError};
```

to:

```rust
pub use loader::{load_pattern, pattern_from_jsonl_line, LoadError};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ssi-scoring`
Expected: PASS — including the two new `jsonl_*` tests and all existing closed-form scorer tests (Invariant 4 untouched).

- [ ] **Step 7: Commit**

```bash
git add ssi-scoring/src/loader.rs ssi-scoring/src/lib.rs
git commit -m "ssi-scoring: add shared JSONL-line -> Pattern reader (diagonal-stripped)"
```

---

## Task 2: I/O wrappers for whole-corpus and single-line loading

**Files:**
- Modify: `ssi-scoring/src/loader.rs` (add two I/O wrappers)
- Modify: `ssi-scoring/src/lib.rs:52` (re-export)
- Test: `ssi-scoring/src/loader.rs` (`#[cfg(test)] mod tests`, append)

**Interfaces:**
- Consumes: `pattern_from_jsonl_line` (Task 1).
- Produces:
  - `pub fn load_corpus_jsonl(path: &Path) -> Result<Vec<(String, Pattern)>, LoadError>` — eager whole-file load, used by the harness.
  - `pub fn load_pattern_jsonl_line(path: &Path, line_index: usize) -> Result<Pattern, LoadError>` — load exactly one line by 0-based index. Not used by the harness; provided so the grader's process-per-matrix worker can address one matrix without parsing the rest.

- [ ] **Step 1: Write the failing test**

Append to `#[cfg(test)] mod tests` in `ssi-scoring/src/loader.rs`:

```rust
#[test]
fn load_corpus_and_single_line_agree() {
    let jsonl = "\
{\"n\":2,\"nnz\":3,\"indptr\":[0,2,3],\"indices\":[0,1,1],\"hash\":\"a\",\"source\":\"m0\"}
{\"n\":4,\"nnz\":12,\"indptr\":[0,3,6,8,12],\"indices\":[0,1,3,0,1,3,2,3,0,1,2,3],\"hash\":\"b\",\"source\":\"m1\"}
";
    let dir = std::env::temp_dir().join("ssi-scoring-jsonl-io-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("patterns.jsonl");
    std::fs::write(&path, jsonl).unwrap();

    let corpus = load_corpus_jsonl(&path).unwrap();
    assert_eq!(corpus.len(), 2);
    assert_eq!(corpus[0].0, "m0");
    assert_eq!(corpus[1].0, "m1");
    assert_eq!(corpus[1].1.n, 4);

    // Single-line load of index 1 equals the whole-corpus entry 1.
    let one = load_pattern_jsonl_line(&path, 1).unwrap();
    assert_eq!(one.col_ptr, corpus[1].1.col_ptr);
    assert_eq!(one.row_idx, corpus[1].1.row_idx);

    // Out-of-range index is an error, not a panic.
    assert!(load_pattern_jsonl_line(&path, 99).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ssi-scoring load_corpus_and_single_line_agree`
Expected: FAIL — `cannot find function load_corpus_jsonl`.

- [ ] **Step 3: Implement the wrappers**

Add to `ssi-scoring/src/loader.rs` (after `pattern_from_jsonl_line`). Note `use std::io::BufRead;` may be needed at the top of the file — add it if the compiler asks:

```rust
/// Load an entire `patterns.jsonl` corpus into `(source, Pattern)` pairs, in
/// file order. Blank lines are skipped. Used by the local harness.
pub fn load_corpus_jsonl(path: &Path) -> Result<Vec<(String, Pattern)>, LoadError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| LoadError::Json(format!("{}: {e}", path.display())))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (source, pat) = pattern_from_jsonl_line(line)
            .map_err(|e| LoadError::Json(format!("{}:{}: {e}", path.display(), i)))?;
        out.push((source, pat));
    }
    Ok(out)
}

/// Load exactly the `line_index`-th (0-based, blank lines counted) pattern from
/// a `patterns.jsonl`. Provided for a future grader worker that grades one
/// matrix per process; the harness uses `load_corpus_jsonl` instead.
pub fn load_pattern_jsonl_line(path: &Path, line_index: usize) -> Result<Pattern, LoadError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| LoadError::Json(format!("{}: {e}", path.display())))?;
    let line = text
        .lines()
        .nth(line_index)
        .ok_or_else(|| LoadError::Json(format!("{}: no line {line_index}", path.display())))?;
    let (_source, pat) = pattern_from_jsonl_line(line)?;
    Ok(pat)
}
```

Add at the top of `ssi-scoring/src/loader.rs` if not present: `use std::path::Path;` (already imported — confirm).

- [ ] **Step 4: Re-export from lib.rs**

In `ssi-scoring/src/lib.rs:52`:

```rust
pub use loader::{
    load_corpus_jsonl, load_pattern, load_pattern_jsonl_line, pattern_from_jsonl_line, LoadError,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ssi-scoring`
Expected: PASS — all loader tests plus closed-form scorer tests green.

- [ ] **Step 6: Commit**

```bash
git add ssi-scoring/src/loader.rs ssi-scoring/src/lib.rs
git commit -m "ssi-scoring: add JSONL corpus I/O wrappers (whole-corpus + single-line)"
```

---

## Task 3: Stage the new corpus and delete the old `.mtx` tree

> **DECISION (2026-06-20, user):** the full ~225 MB corpus is NOT committed.
> Ship only a small in-repo **sample** (13 matrices, ~20 KB: smallest 3 per
> family NLP/QCP/QP/QCQP + `gilbert` n=1001) to exercise the harness pipeline;
> the full corpus is published for download separately. Done inline (mechanical
> data-staging) in commit `7a0a9eb`; a `corpus/dev/README.md` documents the
> sample and where the full corpus lives. The Git-LFS / 225 MB question is moot.

**Files:**
- Delete: `corpus/dev/**/*.mtx` (216 files across `ampl/ bratu/ optctrl/ poisson/ rosenbrock/ sparseqp/`)
- Create: `corpus/dev/patterns.jsonl` (small deterministic sample, not the full file)
- Create: `corpus/dev/README.md` (sample provenance + full-corpus download note)

**Interfaces:**
- Produces: `corpus/dev/patterns.jsonl` at the public-repo path the harness will read in Task 4.

- [ ] **Step 1: Confirm the source corpus exists and is non-empty**

Run: `wc -l ../corpus-generation/corpus/dev/patterns.jsonl`
Expected: `279 ...patterns.jsonl` (non-zero line count).

- [ ] **Step 2: Remove the old `.mtx` dev corpus**

```bash
git rm -r corpus/dev
```

Expected: 216 `.mtx` files staged for deletion.

- [ ] **Step 3: Copy in the new JSONL corpus**

```bash
mkdir -p corpus/dev
cp ../corpus-generation/corpus/dev/patterns.jsonl corpus/dev/patterns.jsonl
git add corpus/dev/patterns.jsonl
```

- [ ] **Step 4: Verify staging**

Run: `git status --short corpus/dev | head`
Expected: many `D corpus/dev/.../*.mtx` lines and one `A corpus/dev/patterns.jsonl`.

> **Note (out of scope, flag to user):** `patterns.jsonl` is ~225 MB. Committing it directly bloats the repo. Confirm with the user whether to use Git LFS or accept the size before the final commit. Do **not** decide unilaterally.

- [ ] **Step 5: Commit**

```bash
git commit -m "corpus: replace .mtx dev tree with patterns.jsonl from corpus-generation"
```

---

## Task 4: Switch the harness `dev_corpus()` to the JSONL reader

**Files:**
- Modify: `src/pattern.rs` (rewrite the file: drop the stdlib `.mtx` reader, point `dev_corpus()` at the shared JSONL reader)
- Modify: `src/main.rs:68-75` (only if the error wording needs to match the new path)

**Interfaces:**
- Consumes: `ssi_scoring::{load_corpus_jsonl, Pattern}`.
- Produces: `pattern::dev_corpus() -> Vec<(String, Pattern)>` (unchanged signature) and `pattern::DEV_CORPUS_FILE` constant; `main.rs` consumes both unchanged.

- [ ] **Step 1: Rewrite `src/pattern.rs`**

Replace the entire contents of `src/pattern.rs` with:

```rust
//! Sparsity patterns and the development corpus loader.
//!
//! HARNESS FILE — do not modify the CONTRACT. The `Pattern` type a contestant
//! ordering sees is `ssi_scoring::Pattern`, re-exported here so the contract
//! signature `order(pattern: &Pattern) -> Vec<usize>` is byte-identical to what
//! the grader scores. It carries structure only — never values, never a
//! right-hand side (NARROW INPUT, proposal §3.1).
//!
//! ## One reader (Invariant 2)
//!
//! The dev corpus is a single `corpus/dev/patterns.jsonl` produced by the
//! `corpus-generation` pipeline: one JSON object per line, each a full
//! symmetrized CSC sparsity pattern. Both this harness and the grader parse a
//! line into a `Pattern` through the SAME function,
//! `ssi_scoring::pattern_from_jsonl_line` — there is no second parser that can
//! silently disagree, so a contestant's local `Pattern` is identical to the
//! graded `Pattern` at the parsing boundary too.

use std::path::PathBuf;

pub use ssi_scoring::Pattern;

/// The shipped public development corpus: one JSONL file of CSC patterns.
pub const DEV_CORPUS_FILE: &str = "corpus/dev/patterns.jsonl";

/// Load the shipped development corpus: every pattern in
/// `corpus/dev/patterns.jsonl`, in file order, named by its `source` problem.
///
/// Parsing goes through `ssi_scoring::load_corpus_jsonl`, the same shared
/// reader the grader builds on (Invariant 2). A malformed corpus is a hard
/// error — the harness must not silently score a partial corpus.
pub fn dev_corpus() -> Vec<(String, Pattern)> {
    let path = PathBuf::from(DEV_CORPUS_FILE);
    match ssi_scoring::load_corpus_jsonl(&path) {
        Ok(corpus) => corpus,
        Err(e) => {
            // Empty vec triggers main.rs's "no matrices found" guard with a
            // clear message; a parse error mid-file is a hard panic (corrupt
            // corpus must never be scored silently).
            if path.exists() {
                panic!("failed to load {}: {e}", path.display());
            }
            Vec::new()
        }
    }
}
```

- [ ] **Step 2: Update the `main.rs` empty-corpus message**

In `src/main.rs`, the empty-corpus guard references `pattern::DEV_CORPUS_DIR`. Change the two references to the new constant. At `src/main.rs:68-75`:

```rust
    let corpus = pattern::dev_corpus();
    if corpus.is_empty() {
        println!(
            "RUN FAILED: no patterns found at {}. Run from the repo root.",
            pattern::DEV_CORPUS_FILE
        );
        std::process::exit(1);
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build --release`
Expected: builds clean. If the compiler reports `DEV_CORPUS_DIR` still referenced anywhere, grep and update: `rg DEV_CORPUS_DIR src/`.

- [ ] **Step 4: Run the harness end-to-end on the real corpus**

Run: `cargo run --release -- --note "jsonl corpus smoke test"`
Expected: prints the per-matrix table (279 rows, names like `st_e09`), a finite `score` line, and writes `score.json` + a `results.tsv` row. No FAIL.

- [ ] **Step 5: Commit**

```bash
git add src/pattern.rs src/main.rs
git commit -m "harness: load dev corpus from patterns.jsonl via shared ssi-scoring reader"
```

---

## Task 5: Delete the dual-reader cross-check test

**Files:**
- Delete: `tests/loader_agreement.rs`

**Interfaces:**
- None. This test only existed to prove the (now-deleted) stdlib `.mtx` reader matched feral's reader. With a single shared JSONL reader, the divergence it guarded against cannot occur; the Task 1/2 unit tests cover the one reader directly.

- [ ] **Step 1: Confirm the test references the deleted reader**

Run: `rg "read_mtx_pattern|harness_pattern" tests/`
Expected: matches only in `tests/loader_agreement.rs` (the reader it imports no longer exists after Task 4, so the test would fail to compile).

- [ ] **Step 2: Delete the test**

```bash
git rm tests/loader_agreement.rs
```

- [ ] **Step 3: Verify the full test suite is green**

Run: `cargo test`
Expected: PASS across the workspace — closed-form scorer tests (Invariant 4), the new `jsonl_*` loader tests, and the remaining `tests/*.rs` (`exact_equivalence`, `narrow_input`, `scorer_crosscheck`). No reference to `loader_agreement`.

- [ ] **Step 4: Commit**

```bash
git rm tests/loader_agreement.rs
git commit -m "tests: drop .mtx dual-reader cross-check (one shared JSONL reader now)"
```

---

## Task 6: Update README and harness docs for the new corpus

**Files:**
- Modify: `README.md` (corpus-format prose, family taxonomy, reference scores)
- Modify: `docs/HARNESS-DESIGN.md` (corpus-loading section, if it describes `.mtx`)

**Interfaces:**
- None (documentation only).

- [ ] **Step 1: Find every stale corpus reference**

Run: `rg -n "\.mtx|MatrixMarket|corpus/dev/|bratu|poisson|optctrl|rosenbrock|sparseqp|n ≈ 160k|160k" README.md docs/`
Expected: a list of spots describing the old `.mtx` tree, the old mesh families, and the old size ceiling.

- [ ] **Step 2: Rewrite the corpus section of README.md**

Update the corpus description to state: the dev corpus is `corpus/dev/patterns.jsonl` (one CSC pattern per line, structure only, no values); families are optimization classes **NLP / QCP / QP / QCQP**; **279** dev patterns spanning `n` from 4 to ~340k. Replace any old `bratu/poisson/...` family list. Note that absolute score numbers were re-measured against this corpus (fill in the value printed by Task 4 Step 4 — do not leave a placeholder).

- [ ] **Step 3: Update the time-cap rationale comment if quoted in docs**

`src/main.rs:50-54` says "the dev corpus reaches n ≈ 160k". The new corpus reaches n ≈ 340k. Update that comment to the new ceiling (the 5 s cap still holds — AMD + symbolic scoring of a ~340k pattern is well under a second). If `docs/HARNESS-DESIGN.md` repeats the figure, update it too.

```rust
/// Per-matrix time cap. The dev corpus reaches n ≈ 340k; AMD + symbolic scoring
/// of the largest matrices is well under a second (Phase 2 §4, cost doc §1),
/// so a cap of 5 s leaves ample room for annealing/learned orderings while
/// killing runaways. (COMPETITION-VERIFIER-COST §1 recommends 2–5 s.)
const TIME_CAP_PER_MATRIX: Duration = Duration::from_secs(5);
```

- [ ] **Step 4: Verify no stale references remain and tests pass**

Run: `rg -n "\.mtx|bratu|poisson" README.md docs/ src/ && cargo test`
Expected: no stale corpus references in docs/README (matches only in unrelated context if any), `cargo test` green.

- [ ] **Step 5: Update the best-score memory**

After Task 4 printed the new score, update the memory file
`/Users/lulu/.claude/projects/-Users-lulu-Documents-autoresearch-ordering-challenge-ssi-ordering-challenge/memory/ordering-best-score.md`
to note the corpus changed (old 0.9801 was on the `.mtx` corpus; re-baseline on the JSONL corpus). Do not invent a number — record the value the harness actually printed.

- [ ] **Step 6: Commit**

```bash
git add README.md docs/HARNESS-DESIGN.md src/main.rs
git commit -m "docs: describe patterns.jsonl corpus + NLP/QCP/QP/QCQP families; re-baseline scores"
```

---

## Self-Review

**Spec coverage:**
- Read new corpus format → Tasks 1–2 (parse core + I/O). ✓
- Local harness consumes it → Task 4. ✓
- One shared reader, reusable by grader → reader lives in `ssi-scoring` with a single-line wrapper shaped for the grader's process-per-matrix model (Task 2, `load_pattern_jsonl_line`); grader wiring explicitly out of scope. ✓
- Diagonal handling (new format includes it, contract drops it) → Task 1 core + its test. ✓
- Invariants: contract frozen (no signature/score/format change), one reader path (Task 5 deletes the dual-reader), submission stdlib-only (no dep added to `ssi-scoring`; stdlib JSON parse), closed-form tests untouched (verified green each task). ✓
- Corpus staging + old tree removal → Task 3. ✓
- Docs/score re-baseline → Task 6. ✓

**Placeholder scan:** No "TBD"/"handle errors appropriately"/"similar to". Score number in README/memory is explicitly "the value the harness printed", not a placeholder to invent. ✓

**Type consistency:** `pattern_from_jsonl_line(&str) -> Result<(String, Pattern), LoadError>`, `load_corpus_jsonl(&Path) -> Result<Vec<(String, Pattern)>, LoadError>`, `load_pattern_jsonl_line(&Path, usize) -> Result<Pattern, LoadError>`, `dev_corpus() -> Vec<(String, Pattern)>` — consistent across Tasks 1, 2, 4. `DEV_CORPUS_FILE` defined in Task 4, used in Task 4 main.rs. `LoadError::Json` defined Task 1, used Tasks 1–2. ✓

**Open items flagged to user (not silently decided):** (a) ~225 MB JSONL in git vs LFS (Task 3 Step 4); (b) grader runner switch to `(jsonl_path, line_index)` — out of scope, reader API ready; (c) empty eval slice needs a real eval-generation run before grading.

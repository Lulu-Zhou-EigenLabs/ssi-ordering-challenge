# Worker Pattern Handoff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the capped worker from re-reading the entire corpus on every invocation by having the parent hand it the already-parsed `Pattern` as a small serialized file.

**Architecture:** The parent already parses the whole corpus into memory at startup. Add a stdlib-only binary serializer (`src/pattern_io.rs`, mirroring `src/perm_io.rs`); the parent writes each matrix's `Pattern` once to the PID-unique scratch dir and passes that file path to the worker instead of `<corpus_path> <line_index>`. The worker deserializes O(one pattern) and never touches the corpus. This lets us delete `ssi_scoring::load_pattern_jsonl_line` and the second full `read_to_string` in `corpus.rs`.

**Tech Stack:** Rust (stdlib only for the new module), `cargo test`, existing `perm_io`/`failsafe`/`watchdog` harness modules.

## Global Constraints

- THE CONTRACT IS FROZEN: do not change the `order()` signature, the score definition, the validity gates, or the `score.json`/`results.tsv` output formats. The parent↔worker wire format is internal and NOT part of the contract. (Invariant 1)
- ONE SCORING CODE PATH: keep `pattern_from_jsonl_line` + `load_corpus_jsonl` as the single JSONL parser shared by harness and grader. (Invariant 2)
- SUBMISSION DIRECTORY STAYS STDLIB-ONLY: `src/pattern_io.rs` is trusted harness code, NOT under `src/ordering/`, and adds no dependency reachable from `src/ordering/`. (Invariant 3)
- CLOSED-FORM TESTS ALWAYS PASS: the scorer's mathematical-fact tests must stay green. (Invariant 4)
- GREEN AND COMMITTED: `cargo test` passes at the end; commit at each working milestone. (Invariant 5)
- `Pattern` fields are all `pub`: `n: usize`, `col_ptr: Vec<usize>`, `row_idx: Vec<usize>` (see `ssi-scoring/src/pattern.rs`). `col_ptr.len() == n + 1`, `col_ptr[0] == 0`, `col_ptr.last() == row_idx.len()`.
- Scratch dir is PID-unique (`ssi-harness-<pid>`) and removed on drop; filenames only need per-run uniqueness. Pattern file = `{seq}-pat.bin`; perm files = `{seq}-a.bin`/`{seq}-b.bin`; `seq` is the `.enumerate()` counter at the call site.

---

### Task 1: `pattern_io` — serialize/deserialize a `Pattern`

**Files:**
- Create: `src/pattern_io.rs`
- Modify: `src/main.rs` (add `mod pattern_io;` next to the other `mod` lines near line 47-52)
- Test: inline `#[cfg(test)]` module in `src/pattern_io.rs`

**Interfaces:**
- Consumes: `ssi_scoring::Pattern` (re-exported at crate root as `crate::Pattern`), with public fields `n`, `col_ptr`, `row_idx`.
- Produces:
  - `pub fn write_pattern(path: &std::path::Path, pat: &crate::Pattern) -> std::io::Result<()>`
  - `pub fn read_pattern(path: &std::path::Path) -> std::io::Result<crate::Pattern>`
- Wire format (all little-endian u64): `n | col_ptr_len | col_ptr[..] | row_idx_len | row_idx[..]`.

- [ ] **Step 1: Add the module declaration**

In `src/main.rs`, add `mod pattern_io;` alongside the existing module declarations (the block `mod corpus; mod failsafe; ...` around lines 47-52). Keep alphabetical-ish grouping; placing it after `mod perm_io;` reads well.

- [ ] **Step 2: Write the failing tests**

Create `src/pattern_io.rs` with ONLY the test module first (the functions don't exist yet, so it won't compile — that is the "fail"). Use the crate-root `Pattern`:

```rust
//! Binary Pattern I/O — the parent→worker handoff for the subprocess-enforced
//! time cap. The parent parses the corpus once, then serializes ONE Pattern per
//! matrix to the scratch dir; the worker reads back just that pattern instead of
//! re-reading the whole corpus (issue #20). Trusted harness code (not scoring,
//! not submission code). `std`-only, mirroring perm_io's shape and guards.
//!
//! Format (all little-endian u64):
//!   n | col_ptr_len | col_ptr[..] | row_idx_len | row_idx[..]
//!
//! read_pattern round-trips the exact bytes the parent already validated — it
//! does NOT re-run Pattern::from_adjacency — so the worker scores the
//! byte-identical Pattern the parent parsed (Invariant 2 at the process
//! boundary). Malformed input is an io::Error, never a panic (like perm_io).

use crate::Pattern;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use ssi_scoring::pattern_from_jsonl_line;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("ssi-harness-pattern-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn roundtrip_pattern() {
        // A real n=4 corpus line (diagonal dropped by the parser).
        let line = r#"{"n":4,"nnz":12,"indptr":[0,3,6,8,12],"indices":[0,1,3,0,1,3,2,3,0,1,2,3],"hash":"x","source":"m"}"#;
        let (_src, pat) = pattern_from_jsonl_line(line).unwrap();
        let path = tmp("roundtrip.bin");
        write_pattern(&path, &pat).unwrap();
        let back = read_pattern(&path).unwrap();
        assert_eq!(back.n, pat.n);
        assert_eq!(back.col_ptr, pat.col_ptr);
        assert_eq!(back.row_idx, pat.row_idx);
    }

    #[test]
    fn roundtrip_empty_pattern() {
        // n=1, no off-diagonal entries: col_ptr=[0,0], row_idx=[].
        let pat = Pattern { n: 1, col_ptr: vec![0, 0], row_idx: vec![] };
        let path = tmp("empty.bin");
        write_pattern(&path, &pat).unwrap();
        let back = read_pattern(&path).unwrap();
        assert_eq!(back.n, 1);
        assert_eq!(back.col_ptr, vec![0, 0]);
        assert!(back.row_idx.is_empty());
    }

    #[test]
    fn read_rejects_truncated() {
        let path = tmp("trunc.bin");
        std::fs::write(&path, [1u8, 2, 3]).unwrap(); // < 8 bytes
        assert!(read_pattern(&path).is_err());
    }

    #[test]
    fn read_rejects_overflow_len() {
        // n=1, then a col_ptr_len of u64::MAX — must not attempt a huge alloc.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&u64::MAX.to_le_bytes());
        let path = tmp("overflow.bin");
        std::fs::write(&path, bytes).unwrap();
        assert!(read_pattern(&path).is_err());
    }

    #[test]
    fn read_rejects_structural_inconsistency() {
        // Well-formed length prefixes, but col_ptr.last() != row_idx.len().
        let pat = Pattern { n: 2, col_ptr: vec![0, 1, 2], row_idx: vec![1, 0] };
        let path = tmp("bad-struct.bin");
        write_pattern(&path, &pat).unwrap();
        // Corrupt row_idx_len to 1 by rewriting the bytes: easier to hand-build.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u64.to_le_bytes()); // n
        bytes.extend_from_slice(&3u64.to_le_bytes()); // col_ptr_len
        for v in [0u64, 1, 2] { bytes.extend_from_slice(&v.to_le_bytes()); }
        bytes.extend_from_slice(&1u64.to_le_bytes()); // row_idx_len = 1 (inconsistent: col_ptr says 2)
        bytes.extend_from_slice(&0u64.to_le_bytes()); // one row idx
        std::fs::write(&path, bytes).unwrap();
        assert!(read_pattern(&path).is_err());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p ssi-ordering-challenge --bin ssi-ordering-challenge pattern_io`
Expected: FAIL — compile error, `write_pattern`/`read_pattern` not found.

- [ ] **Step 4: Write the implementation**

Add above the `#[cfg(test)]` module in `src/pattern_io.rs`:

```rust
pub fn write_pattern(path: &Path, pat: &Pattern) -> std::io::Result<()> {
    let total = 8 + 8 + pat.col_ptr.len() * 8 + 8 + pat.row_idx.len() * 8;
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&(pat.n as u64).to_le_bytes());
    buf.extend_from_slice(&(pat.col_ptr.len() as u64).to_le_bytes());
    for &v in &pat.col_ptr {
        buf.extend_from_slice(&(v as u64).to_le_bytes());
    }
    buf.extend_from_slice(&(pat.row_idx.len() as u64).to_le_bytes());
    for &v in &pat.row_idx {
        buf.extend_from_slice(&(v as u64).to_le_bytes());
    }
    let mut f = fs::File::create(path)?;
    f.write_all(&buf)
}

pub fn read_pattern(path: &Path) -> std::io::Result<Pattern> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.read_to_end(&mut bytes)?;
    let err = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());

    let mut cur = 0usize;
    let mut read_u64 = |cur: &mut usize| -> std::io::Result<u64> {
        let end = cur.checked_add(8).ok_or_else(|| err("offset overflow"))?;
        if end > bytes.len() {
            return Err(err("truncated pattern file"));
        }
        let v = u64::from_le_bytes(bytes[*cur..end].try_into().unwrap());
        *cur = end;
        Ok(v)
    };

    let n = read_u64(&mut cur)? as usize;

    let col_ptr_len = read_u64(&mut cur)? as usize;
    let mut col_ptr = Vec::with_capacity(col_ptr_len.min(bytes.len() / 8));
    for _ in 0..col_ptr_len {
        col_ptr.push(read_u64(&mut cur)? as usize);
    }

    let row_idx_len = read_u64(&mut cur)? as usize;
    let mut row_idx = Vec::with_capacity(row_idx_len.min(bytes.len() / 8));
    for _ in 0..row_idx_len {
        row_idx.push(read_u64(&mut cur)? as usize);
    }

    if cur != bytes.len() {
        return Err(err("trailing bytes after pattern"));
    }
    // Structural sanity — cheap, and matches Pattern's documented invariants.
    if col_ptr.len() != n + 1 {
        return Err(err("col_ptr length != n+1"));
    }
    if col_ptr.first() != Some(&0) {
        return Err(err("col_ptr[0] != 0"));
    }
    if col_ptr.last().copied() != Some(row_idx.len()) {
        return Err(err("col_ptr[n] != row_idx.len()"));
    }
    Ok(Pattern { n, col_ptr, row_idx })
}
```

Note: `Vec::with_capacity(len.min(bytes.len() / 8))` bounds the pre-allocation by the bytes actually present, so a bogus huge `len` cannot trigger a giant allocation before the truncation check fires.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ssi-ordering-challenge --bin ssi-ordering-challenge pattern_io`
Expected: PASS — all five tests green.

- [ ] **Step 6: Commit**

```bash
git add src/pattern_io.rs src/main.rs
git commit -m "feat(pattern_io): binary Pattern serializer for parent->worker handoff (#20)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Switch the handoff — worker, parent, and corpus loader together

These three changes are one atomic switch: the worker CLI, the parent's
serialize+loop, and the corpus loader's return type each break compilation
without the others, so they land in a single task and are tested together.

**Files:**
- Modify: `src/main.rs` — `worker()` (lines ~305-329), the per-matrix loop / `run_once` / `jsonl_path` binding (lines ~103, ~132-181)
- Modify: `src/corpus.rs` — `corpus_indexed()` → `corpus()` (lines ~72-116) and its tests (lines ~144-157)

**Interfaces:**
- Consumes: `pattern_io::write_pattern`, `pattern_io::read_pattern` (Task 1); `ssi_scoring::load_corpus_jsonl` (unchanged single parser).
- Produces:
  - worker CLI `--worker <pattern_file> <out_perm>`; parent writes `{seq}-pat.bin` per matrix.
  - `pub fn corpus() -> Vec<(String, Pattern)>` (renamed from `corpus_indexed`; drops the raw-index element and the second full read).

- [ ] **Step 1: Rewrite the corpus loader test first (TDD)**

In `src/corpus.rs` tests, replace `indexed_corpus_matches_single_line_loader` (which cross-checks the soon-deleted single-line loader) with a test that the renamed loader agrees with `load_corpus_jsonl` in order:

```rust
    #[test]
    fn corpus_matches_shared_jsonl_reader() {
        // The harness corpus loader must yield exactly what the shared
        // ssi_scoring reader parses, in the same order — one parser (Invariant 2).
        let path = std::path::Path::new(DEV_CORPUS_FILE);
        let shared = ssi_scoring::load_corpus_jsonl(path).expect("load dev corpus");
        let ours = corpus();
        assert_eq!(ours.len(), shared.len());
        for ((src_a, pat_a), (src_b, pat_b)) in ours.iter().zip(shared.iter()) {
            assert_eq!(src_a, src_b);
            assert_eq!(pat_a.n, pat_b.n);
            assert_eq!(pat_a.col_ptr, pat_b.col_ptr);
            assert_eq!(pat_a.row_idx, pat_b.row_idx);
        }
    }
```

- [ ] **Step 2: Rewrite the corpus loader**

Replace `corpus_indexed()` (the whole function, lines ~72-116 including its doc comment) with:

```rust
/// Load the development corpus as `(source, Pattern)` pairs, in file order,
/// through the shared `ssi_scoring::load_corpus_jsonl` reader (the single JSONL
/// parser — Invariant 2). Returns an empty vec when the corpus file is absent so
/// the caller can print an actionable "run from the repo root" message; panics
/// on a present-but-unparseable file or an unresolved Git LFS pointer.
pub fn corpus() -> Vec<(String, Pattern)> {
    let path = corpus_path();
    // If the corpus is an unresolved Git LFS pointer (clone without git-lfs),
    // fail loudly with a fix, not a confusing parse error. Only the first line
    // matters (the pointer's `version` marker), so read a small prefix rather
    // than slurping the whole corpus (~99 MB) just to check it.
    if is_lfs_pointer(&read_prefix(&path)) {
        panic!(
            "{} is an unresolved Git LFS pointer, not the corpus.\n\
             Install git-lfs and fetch the real file:\n\
             \x20   git lfs install && git lfs pull",
            path.display()
        );
    }
    match ssi_scoring::load_corpus_jsonl(&path) {
        Ok(c) => c,
        Err(e) => {
            if path.exists() {
                panic!("failed to load {}: {e}", path.display());
            }
            Vec::new()
        }
    }
}
```

This removes the second `std::fs::read_to_string`, the `raw_indices` recovery, the `zip`, and the `debug_assert_eq!`.

- [ ] **Step 3: Change the worker to read a pattern file**

Replace the body of `worker()` (currently parses `<jsonl_path> <line_index> <out_perm>` and calls `ssi_scoring::load_pattern_jsonl_line`) with a two-arg version. New `worker()`:

```rust
/// `--worker <pattern_file> <out_perm>`: read the one pattern the parent
/// serialized for this matrix, run the contestant order(), write the
/// permutation. The parent supervises this under the time cap and SIGKILLs it on
/// breach. A panic aborts the process (non-zero exit, no perm file) — the parent
/// treats that as a FAIL.
fn worker(args: &[String]) -> i32 {
    let (Some(pattern_file), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("--worker: usage: --worker <pattern_file> <out_perm>");
        return 2;
    };
    let pat = match pattern_io::read_pattern(Path::new(pattern_file)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("worker: failed to read pattern from {pattern_file}: {e}");
            return 3;
        }
    };
    let perm = ordering::order(&pat);
    match perm_io::write_perm(Path::new(out), &perm) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("worker: failed to write perm to {out}: {e}");
            4
        }
    }
}
```

- [ ] **Step 4: Change the parent to serialize the pattern and pass its path**

In `main()`, change the call site `let corpus = corpus::corpus_indexed();` (line ~103) to `let corpus = corpus::corpus();`. The corpus loop header currently is `for (line_index, name, pat) in &corpus {`; now that the corpus is `Vec<(String, Pattern)>`, switch to an enumerated loop and serialize once per matrix. Also delete the now-unused `jsonl_path` binding (currently around line 132) and its comment.

Replace the loop opening + `run_once` closure. The loop header becomes:

```rust
    for (seq, (name, pat)) in corpus.iter().enumerate() {
```

Immediately after the AMD-baseline block (which ends at the `};` closing the `let base = match ...`), before the `run_once` closure, add the one-time serialize:

```rust
        // Serialize THIS pattern once to the scratch dir; both determinism runs
        // read it. Written by the trusted parent OUTSIDE the timed window, so its
        // cost never counts against the per-matrix cap.
        let pat_file = scratch.path().join(format!("{seq}-pat.bin"));
        let _ = std::fs::remove_file(&pat_file);
        if let Err(e) = pattern_io::write_pattern(&pat_file, pat) {
            failed = Some(format!("{name}: failed to stage pattern for worker: {e}"));
            break;
        }
```

Then the `run_once` closure changes its output-file stem from `line_index` to `seq` and its command args from `<jsonl_path> <line_index>` to `<pat_file>`:

```rust
        let run_once = |tag: &str| -> Result<Vec<usize>, String> {
            let out_perm = scratch.path().join(format!("{seq}-{tag}.bin"));
            let _ = std::fs::remove_file(&out_perm);
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--worker").arg(&pat_file).arg(&out_perm);
            let t0 = Instant::now();
            match watchdog::run_capped(&mut cmd, &cap) {
                // ... unchanged Ok/Timeout/Crashed arms ...
            }
        };
```

Leave the Ok/Timeout/Crashed match arms exactly as they are.

- [ ] **Step 5: Delete the now-dead parent bindings**

Remove the `let jsonl_path = corpus_file.to_string_lossy().into_owned();` line and the two-line comment above it ("The worker must load from the SAME corpus file …"). `corpus_file` is still used earlier for the empty-corpus error message, so keep that binding.

- [ ] **Step 6: Run the corpus loader test + full build**

Run: `cargo test -p ssi-ordering-challenge --bin ssi-ordering-challenge corpus`
Expected: PASS — `corpus_matches_shared_jsonl_reader`, the three `corpus_path*` tests, and `detects_git_lfs_pointer_text` all green.

Run: `cargo build -p ssi-ordering-challenge`
Expected: compiles. If `ssi_scoring::load_pattern_jsonl_line` is still imported/used anywhere it will error — that is removed in Task 3; a leftover reference here means a call site was missed.

- [ ] **Step 7: End-to-end test — the real spawn path**

Run: `cargo test -p ssi-ordering-challenge --test time_cap`
Expected: PASS — `normal_run_succeeds_on_sample_corpus` (worker now reads the pattern file) and `slow_ordering_is_killed_and_fails_promptly` both green.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/corpus.rs
git commit -m "fix(#20): hand worker the parsed pattern instead of a corpus path+index

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Delete `load_pattern_jsonl_line` from `ssi-scoring`

**Files:**
- Modify: `ssi-scoring/src/loader.rs` — delete the function (lines ~134-152) and adapt tests (lines ~182-206)
- Modify: `ssi-scoring/src/lib.rs` — remove from the `pub use loader::{...}` list (line ~54)

**Interfaces:**
- Removes: `ssi_scoring::load_pattern_jsonl_line` (no remaining callers after Task 2).
- Keeps: `pattern_from_jsonl_line`, `load_corpus_jsonl`, `LoadError`, `Pattern`.

- [ ] **Step 1: Remove the export**

In `ssi-scoring/src/lib.rs`, change:

```rust
pub use loader::{
    load_corpus_jsonl, load_pattern_jsonl_line, pattern_from_jsonl_line, LoadError,
};
```

to:

```rust
pub use loader::{load_corpus_jsonl, pattern_from_jsonl_line, LoadError};
```

- [ ] **Step 2: Delete the function**

In `ssi-scoring/src/loader.rs`, delete `load_pattern_jsonl_line` entirely — the doc comment starting `/// Load exactly the ...` through the closing `}` (currently lines ~134-152).

- [ ] **Step 3: Adapt the loader tests**

In `loader.rs`'s `load_corpus_and_single_line_agree` test, remove the two lines that call `load_pattern_jsonl_line` and its two out-of-range assertions:

```rust
        // Single-line load of index 1 equals the whole-corpus entry 1.
        let one = load_pattern_jsonl_line(&path, 1).unwrap();
        assert_eq!(one.col_ptr, corpus[1].1.col_ptr);
        assert_eq!(one.row_idx, corpus[1].1.row_idx);

        // Out-of-range index is an error, not a panic.
        assert!(load_pattern_jsonl_line(&path, 99).is_err());
```

Delete those lines. The remaining assertions (corpus length, sources, `corpus[1].1.n`) still validate `load_corpus_jsonl`. Rename the test to `load_corpus_parses_all_lines` to match what it now covers.

- [ ] **Step 4: Run the scoring crate tests**

Run: `cargo test -p ssi-scoring`
Expected: PASS — no reference to the deleted function; parser and closed-form scorer tests green.

- [ ] **Step 5: Commit**

```bash
git add ssi-scoring/src/loader.rs ssi-scoring/src/lib.rs
git commit -m "refactor(#20): delete unused load_pattern_jsonl_line (blank-line index footgun)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Full verification

**Files:** none (verification only).

- [ ] **Step 1: Whole workspace test**

Run: `cargo test`
Expected: PASS across `ssi-scoring`, the harness bin, and `tests/time_cap.rs`. (Invariant 4 closed-form tests + Invariant 5 green.)

- [ ] **Step 2: Confirm the loader is gone and no stragglers reference it**

Run: `grep -rn --include="*.rs" load_pattern_jsonl_line src ssi-scoring`
Expected: no matches.

- [ ] **Step 3: Real run over the dev corpus**

Run: `cargo run --release -- --note "issue #20: worker pattern handoff"`
Expected: prints the per-matrix table + per-bucket breakdown, writes `score.json` and appends an `OK` row to `results.tsv`; exits 0. (Confirms the end-to-end handoff produces a valid scored run.)

- [ ] **Step 4: Confirm score.json unchanged in shape**

Run: `cat score.json`
Expected: same top-level `score` + `metrics` structure as before (contract output format unchanged — Invariant 1).

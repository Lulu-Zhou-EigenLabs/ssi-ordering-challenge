# Bucketed Weighted Scoring + Full-Corpus Git LFS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change the harness score from a single global geomean to a size-bucketed, weighted mean of per-bucket geomeans, and ship the real 300-pattern dev corpus in-repo via Git LFS as the default corpus.

**Architecture:** All scoring aggregation lives in one place — `src/main.rs` (Yukon dispatches this same binary, so there is no second scoring path). Per-matrix ratio computation in `ssi-scoring` is unchanged. The corpus change is packaging only: a `.gitattributes` LFS rule, the 99 MB corpus committed as an LFS object at the default path, an LFS-pointer guard in the loader, and `lfs: true` on the CI checkout.

**Tech Stack:** Rust (stdlib + feral via ssi-scoring), Git LFS, GitHub Actions.

## Global Constraints

- **Invariant 1 (contract):** `order()` signature, the validity gates, and the `score.json` / `results.tsv` output *formats* (column layout) must NOT change. The score *definition* is being deliberately revised (owner-authorized); re-baseline and update all docs. Absolute score values may change; definitions of gates/signature/formats may not.
- **Invariant 2 (one scoring path):** Do not add a second scorer. All aggregation stays in `src/main.rs`; per-matrix scoring stays in `ssi_scoring::score`.
- **Invariant 3 (submission dir stdlib-only):** Do NOT touch `src/ordering/`. No new `[dependencies]`.
- **Invariant 4 (closed-form tests):** `ssi-scoring` closed-form tests must keep passing untouched — per-matrix math does not change.
- **Invariant 5 (green + committed):** `cargo test` passes at the end of every task; commit at every working milestone.
- **Bucket bounds (half-open):** `lt_1k = n < 1000`, `1k_10k = 1000 ≤ n < 10000`, `gt_10k = n ≥ 10000`.
- **Weights:** `lt_1k = 0.30`, `1k_10k = 0.30`, `gt_10k = 0.40`.
- **Empty buckets:** renormalize — weighted mean over populated buckets only, weights rescaled to sum to 1.
- **Bucket metric keys:** `lt_1k` / `1k_10k` / `gt_10k`.
- **Canonical full corpus source:** `../corpus-generation/corpus/dev/patterns.jsonl` (absolute: `/Users/lulu/Documents/autoresearch/ordering_challenge/corpus-generation/corpus/dev/patterns.jsonl`), ~99 MB, 300 patterns, bucket split `lt_1k=147, 1k_10k=108, gt_10k=45`.
- **Commit trailer:** end every commit message with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- `src/main.rs` — MODIFY. Add bucket classification + weighted-combine helpers, per-bucket accumulation in the scoring loop, the new terminal output, the new `score.json` `metrics` payload, and unit tests. This is the only code file with behavior change.
- `src/corpus.rs` — MODIFY. Add a Git LFS pointer-file guard so a missing-`git-lfs` clone gets a clear "run `git lfs pull`" message instead of an opaque parse error. Add a unit test.
- `.gitattributes` — CREATE. LFS tracking rule for the corpus.
- `corpus/dev/patterns.jsonl` — REPLACE (via LFS). The 13-pattern sample → the 300-pattern full corpus, stored as an LFS object.
- `.github/workflows/benchmark.yml` — MODIFY. `lfs: true` on the checkout step.
- `README.md`, `RULES.md`, `corpus/dev/README.md` — MODIFY. Score description → bucketed; corpus section → LFS story (remove dead release-asset links).
- `benchmark.json` — MODIFY. `description` field text.
- `results.tsv` — MODIFY. Header comment + starter row note.

Tasks 1–3 (scoring) are independent of Tasks 4–6 (corpus/LFS) except that the re-baseline in Task 3 and the doc updates in Task 7 depend on both. Scoring is done first because it is testable on the existing 13-line sample without LFS.

---

## Task 1: Bucket classification + weighted-combine helpers

Pure helpers with unit tests. No wiring into the loop yet — this task delivers the two functions the aggregation will use, fully tested in isolation.

**Files:**
- Modify: `src/main.rs` (add helpers + a `#[cfg(test)]` module)

**Interfaces:**
- Consumes: nothing (pure stdlib).
- Produces:
  - `const BUCKETS: usize = 3;`
  - `const BUCKET_KEYS: [&str; 3] = ["lt_1k", "1k_10k", "gt_10k"];`
  - `const BUCKET_WEIGHTS: [f64; 3] = [0.30, 0.30, 0.40];`
  - `fn size_bucket(n: usize) -> usize` — returns bucket index `0|1|2` for `n`.
  - `struct BucketAcc { log_ratio_sum: f64, log_fill_sum: f64, count: usize }` with `Default`.
  - `fn geomean(log_sum: f64, count: usize) -> Option<f64>` — `None` when `count == 0`, else `exp(log_sum / count)`.
  - `fn combine(geomeans: &[Option<f64>; 3], weights: &[f64; 3]) -> f64` — renormalized weighted mean over populated (`Some`) buckets; returns `f64::NAN` if all buckets empty.

- [ ] **Step 1: Write the failing tests**

Add to `src/main.rs` at the end of the file:

```rust
#[cfg(test)]
mod scoring_tests {
    use super::*;

    #[test]
    fn size_bucket_boundaries() {
        assert_eq!(size_bucket(0), 0);
        assert_eq!(size_bucket(999), 0);
        assert_eq!(size_bucket(1000), 1);
        assert_eq!(size_bucket(9999), 1);
        assert_eq!(size_bucket(10000), 2);
        assert_eq!(size_bucket(340_000), 2);
    }

    #[test]
    fn geomean_empty_is_none() {
        assert_eq!(geomean(0.0, 0), None);
    }

    #[test]
    fn geomean_matches_exp_mean() {
        // two ratios 0.5 and 0.8 → geomean = sqrt(0.4) ≈ 0.632455
        let ls = 0.5_f64.ln() + 0.8_f64.ln();
        let g = geomean(ls, 2).unwrap();
        assert!((g - (0.4_f64).sqrt()).abs() < 1e-12, "g = {g}");
    }

    #[test]
    fn combine_all_populated_matches_worked_example() {
        // user's example: 0.8, 0.9, 0.7 with weights 0.3, 0.3, 0.4
        let gms = [Some(0.8), Some(0.9), Some(0.7)];
        let got = combine(&gms, &BUCKET_WEIGHTS);
        let want = 0.30 * 0.8 + 0.30 * 0.9 + 0.40 * 0.7;
        assert!((got - want).abs() < 1e-12, "got = {got}, want = {want}");
    }

    #[test]
    fn combine_one_empty_renormalizes() {
        // lt_1k empty → weighted mean over {1k_10k: 0.9, gt_10k: 0.7} with
        // weights {0.3, 0.4} renormalized by 0.7.
        let gms = [None, Some(0.9), Some(0.7)];
        let got = combine(&gms, &BUCKET_WEIGHTS);
        let want = (0.30 * 0.9 + 0.40 * 0.7) / (0.30 + 0.40);
        assert!((got - want).abs() < 1e-12, "got = {got}, want = {want}");
    }

    #[test]
    fn combine_only_one_populated_is_that_geomean() {
        // dev-corpus case: only lt_1k populated → score == its geomean.
        let gms = [Some(0.873), None, None];
        let got = combine(&gms, &BUCKET_WEIGHTS);
        assert!((got - 0.873).abs() < 1e-12, "got = {got}");
    }

    #[test]
    fn combine_all_empty_is_nan() {
        let gms = [None, None, None];
        assert!(combine(&gms, &BUCKET_WEIGHTS).is_nan());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin ssi-ordering-challenge scoring_tests 2>&1 | tail -20`
Expected: FAIL — `cannot find function size_bucket` / `geomean` / `combine` / `BUCKET_WEIGHTS not found`.

(If the bin name differs, `cargo test scoring_tests` also works; confirm the bin name with `cargo metadata --no-deps --format-version 1 | grep -o '"name":"[^"]*"' | head`.)

- [ ] **Step 3: Write minimal implementation**

Add near the top of `src/main.rs`, after the `use` block and the `TIME_CAP_PER_MATRIX` const:

```rust
/// Number of size buckets the score is aggregated over.
const BUCKETS: usize = 3;
/// Stable metric keys for the buckets, in index order (see `size_bucket`).
const BUCKET_KEYS: [&str; BUCKETS] = ["lt_1k", "1k_10k", "gt_10k"];
/// Weights per bucket. Real-world value and algorithmic difficulty concentrate
/// in the large matrices, so `gt_10k` carries the most weight. Empty buckets are
/// renormalized out in `combine`, so these need not be pre-normalized.
const BUCKET_WEIGHTS: [f64; BUCKETS] = [0.30, 0.30, 0.40];

/// Classify a matrix by its dimension `n` into a bucket index (half-open):
/// `n < 1000 → 0` (lt_1k), `1000 ≤ n < 10000 → 1` (1k_10k), `n ≥ 10000 → 2` (gt_10k).
fn size_bucket(n: usize) -> usize {
    if n < 1_000 {
        0
    } else if n < 10_000 {
        1
    } else {
        2
    }
}

/// Per-bucket accumulator: sums of log-ratios (for the geomean) and a count.
#[derive(Default, Clone, Copy)]
struct BucketAcc {
    log_ratio_sum: f64,
    log_fill_sum: f64,
    count: usize,
}

/// Geometric mean from a sum of natural logs and a count. `None` for an empty
/// bucket (no matrices), so `combine` can renormalize it out.
fn geomean(log_sum: f64, count: usize) -> Option<f64> {
    if count == 0 {
        None
    } else {
        Some((log_sum / count as f64).exp())
    }
}

/// Weighted mean of the per-bucket geomeans, renormalizing the weights over the
/// populated (`Some`) buckets. Returns `NaN` if every bucket is empty.
fn combine(geomeans: &[Option<f64>; BUCKETS], weights: &[f64; BUCKETS]) -> f64 {
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for i in 0..BUCKETS {
        if let Some(g) = geomeans[i] {
            num += weights[i] * g;
            den += weights[i];
        }
    }
    if den == 0.0 {
        f64::NAN
    } else {
        num / den
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin ssi-ordering-challenge scoring_tests 2>&1 | tail -20`
Expected: PASS — all 7 tests in `scoring_tests`.

Note: `BucketAcc` is unused until Task 2; expect a dead-code warning on it (and possibly on `BUCKET_KEYS`). That is fine within this task — Task 2 consumes both. If the crate is built with `-D warnings` anywhere, add `#[allow(dead_code)]` on `BucketAcc` and `BUCKET_KEYS` here and remove it in Task 2. (Check: `grep -rn "deny(warnings)\|-D warnings" Cargo.toml.in .cargo/ 2>/dev/null` — none expected.)

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(scoring): add size-bucket classification + weighted-combine helpers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Wire bucketed aggregation into the scoring loop + new output

Replace the two scalar accumulators with per-bucket accumulation, compute the weighted score and tiebreak, print the per-bucket breakdown, and write the extended `score.json`. The `results.tsv` column layout is unchanged (only the values' meaning changes).

**Files:**
- Modify: `src/main.rs:102-207` (the accumulators, the loop body, and the post-loop `match failed` block)

**Interfaces:**
- Consumes: `size_bucket`, `BucketAcc`, `geomean`, `combine`, `BUCKETS`, `BUCKET_KEYS`, `BUCKET_WEIGHTS` (Task 1); `ssi_scoring::score` (unchanged); `pat.n`.
- Produces: new `score.json` shape with `metrics.buckets` + `metrics.weights`; unchanged `append_results` call signature.

- [ ] **Step 1: Replace the scalar accumulators**

In `src/main.rs`, replace:

```rust
    let mut log_ratio_sum = 0.0_f64;
    let mut log_fill_sum = 0.0_f64;
    let mut failed: Option<String> = None;
    let mut table = String::new();
```

with:

```rust
    let mut buckets = [BucketAcc::default(); BUCKETS];
    let mut failed: Option<String> = None;
    let mut table = String::new();
```

- [ ] **Step 2: Accumulate into the matching bucket in the loop**

In the loop body, replace:

```rust
        log_ratio_sum += ratio.ln();
        log_fill_sum += fill_ratio.ln();
```

with:

```rust
        let b = size_bucket(pat.n);
        buckets[b].log_ratio_sum += ratio.ln();
        buckets[b].log_fill_sum += fill_ratio.ln();
        buckets[b].count += 1;
```

- [ ] **Step 3: Replace the success branch (combine, print, write score.json)**

Replace the entire `None => { ... }` arm of `match failed` with:

```rust
        None => {
            // Per-bucket geomeans (None for an empty bucket).
            let flop_gms: [Option<f64>; BUCKETS] =
                std::array::from_fn(|i| geomean(buckets[i].log_ratio_sum, buckets[i].count));
            let fill_gms: [Option<f64>; BUCKETS] =
                std::array::from_fn(|i| geomean(buckets[i].log_fill_sum, buckets[i].count));

            let score_val = combine(&flop_gms, &BUCKET_WEIGHTS);
            let fill = combine(&fill_gms, &BUCKET_WEIGHTS);

            // Per-bucket breakdown table.
            println!("\nper-bucket (geomean of ratio vs AMD, within bucket):");
            println!(
                "{:<8} {:>6} {:>16} {:>16}",
                "bucket", "count", "flop_geomean", "fill_geomean"
            );
            for i in 0..BUCKETS {
                let fmt = |g: Option<f64>| g.map_or_else(|| "—".to_string(), |v| format!("{v:.4}"));
                println!(
                    "{:<8} {:>6} {:>16} {:>16}",
                    BUCKET_KEYS[i],
                    buckets[i].count,
                    fmt(flop_gms[i]),
                    fmt(fill_gms[i]),
                );
            }

            println!(
                "\nscore (weighted mean of per-bucket geomean flop ratios, lower is better): {score_val:.4}"
            );
            println!("tiebreak (weighted mean of per-bucket geomean fill ratios):                    {fill:.4}");

            // score.json — top-level `score` is what the grader ranks on;
            // `metrics` is passthrough detail (Yukon captures it whole and shows
            // it in the PR report). All three buckets are always listed; an empty
            // bucket has count 0 and null geomeans.
            let mut buckets_json = String::new();
            for i in 0..BUCKETS {
                let jf = |g: Option<f64>| g.map_or_else(|| "null".to_string(), |v| format!("{v:.6}"));
                let sep = if i + 1 < BUCKETS { "," } else { "" };
                let _ = write!(
                    buckets_json,
                    "\"{}\": {{ \"count\": {}, \"geomean_flop_ratio\": {}, \"geomean_fill_ratio\": {} }}{}",
                    BUCKET_KEYS[i],
                    buckets[i].count,
                    jf(flop_gms[i]),
                    jf(fill_gms[i]),
                    sep,
                );
            }
            let total: usize = buckets.iter().map(|b| b.count).sum();
            let json = format!(
                "{{ \"score\": {score_val:.6}, \"metrics\": {{ \
                 \"geomean_flop_ratio\": {score_val:.6}, \
                 \"geomean_fill_ratio\": {fill:.6}, \
                 \"matrices\": {total}, \
                 \"weights\": {{ \"lt_1k\": {:.2}, \"1k_10k\": {:.2}, \"gt_10k\": {:.2} }}, \
                 \"buckets\": {{ {buckets_json} }} }} }}\n",
                BUCKET_WEIGHTS[0], BUCKET_WEIGHTS[1], BUCKET_WEIGHTS[2],
            );
            std::fs::write("score.json", json).expect("write score.json");
            append_results(timestamp, "OK", score_val, fill, &note);
        }
```

Note: `write!` on a `String` requires `use std::fmt::Write as _;`, which `main.rs` already imports (line 44). Remove the now-unused `let m = corpus.len() as f64;` line if it remains.

- [ ] **Step 4: Build and run against the in-repo sample**

Run: `cargo run --release 2>&1 | tail -25`
Expected: the per-matrix table, then a per-bucket table where `lt_1k` shows count 13 (or 12 + the n=1001 `gilbert` case in `1k_10k` — verify against `awk` below), the other buckets show `—`, and a final score line. The 13-line sample includes `gilbert` at n=1001, so expect `lt_1k` and `1k_10k` populated, `gt_10k` empty.

Verify the sample's bucket split:
Run: `awk -F'"n":' '{split($2,a,","); n=a[1]; if(n<1000)s++; else if(n<10000)m++; else l++} END{print "lt_1k="s" 1k_10k="m" gt_10k="l}' corpus/dev/patterns.jsonl`
Expected: `lt_1k=12 1k_10k=1 gt_10k=0` (12 tiny + gilbert n=1001).

- [ ] **Step 5: Verify score.json shape**

Run: `cat score.json`
Expected: valid JSON with top-level `score`, and `metrics.buckets` listing all three keys — `gt_10k` with `"count": 0, "geomean_flop_ratio": null, "geomean_fill_ratio": null`.

Sanity-check it parses:
Run: `python3 -c "import json; d=json.load(open('score.json')); assert set(d['metrics']['buckets'])=={'lt_1k','1k_10k','gt_10k'}; assert d['metrics']['buckets']['gt_10k']['count']==0; print('ok', d['score'])"`
Expected: `ok <number>`.

- [ ] **Step 6: Run the full test suite**

Run: `cargo test 2>&1 | tail -25`
Expected: PASS — `scoring_tests`, plus all existing tests (`ssi-scoring` closed-form, `scorer_crosscheck`, `corpus`, `time_cap`) unchanged.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat(scoring): bucket by size and weight per-bucket geomeans

Score is now the renormalized weighted mean (0.30/0.30/0.40) of the
per-bucket geomean flop ratios; same scheme for the fill tiebreak.
score.json metrics gains per-bucket counts + geomeans and the weights.
results.tsv layout unchanged. One scoring path preserved (Invariant 2).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Update the main.rs scoring docstring

Doc-only: the module docstring still says `score = geometric mean over the corpus`. Correct it to the bucketed definition so the contract doc matches behavior (Invariant 1 handling).

**Files:**
- Modify: `src/main.rs:20-32` (module docstring scoring block)

**Interfaces:** none.

- [ ] **Step 1: Rewrite the scoring block in the docstring**

Replace lines 20-32 (the block from `//! then prints a per-matrix table, computes` through the `//! same matrices ...` paragraph) with:

```rust
//! then prints a per-matrix table and a per-bucket breakdown, computes
//!
//!     score = weighted mean over size buckets of
//!             geomean_within_bucket( flops(yours) / flops(AMD) )
//!             (lower is better)
//!
//! Matrices are bucketed by dimension n — lt_1k (n<1000), 1k_10k
//! (1000≤n<10000), gt_10k (n≥10000) — with weights 0.30 / 0.30 / 0.40. Empty
//! buckets are renormalized out (weights rescaled over populated buckets), so on
//! a corpus that only populates one bucket the score is just that bucket's
//! geomean. The tiebreak is the same weighted scheme over the fill ratio
//! nnz(L)(yours)/nnz(L)(AMD).
//!
//! ONE SCORING CODE PATH (Invariant 2): the baseline and your ordering are both
//! scored by `ssi_scoring::score`, the same function the private grader calls,
//! and the aggregation above lives only here. The per-matrix score is a pure
//! function of (pattern, permutation), so the number printed here is IDENTICAL
//! to the number the grader computes for the same ordering on the same matrices.
```

- [ ] **Step 2: Verify it still builds**

Run: `cargo build --release 2>&1 | tail -5`
Expected: builds clean (docstring change only).

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "docs(scoring): describe bucketed weighted score in main.rs docstring

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Git LFS pointer-file guard in the corpus loader

Before shipping the LFS corpus, add a guard so a clone WITHOUT `git-lfs` (which yields the ~130-byte pointer text instead of JSONL) fails with a clear, actionable message instead of an opaque JSON parse error. TDD the detector; wire it into the corpus load path.

**Files:**
- Modify: `src/corpus.rs` (add detector + call it in `corpus_indexed` before parsing; add a unit test)

**Interfaces:**
- Consumes: nothing new.
- Produces: `fn is_lfs_pointer(text: &str) -> bool` (module-private).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/corpus.rs`:

```rust
    #[test]
    fn detects_git_lfs_pointer_text() {
        let pointer = "version https://git-lfs.github.com/spec/v1\n\
                       oid sha256:abc123\n\
                       size 103879806\n";
        assert!(is_lfs_pointer(pointer));
        // A real JSONL corpus line is not a pointer.
        assert!(!is_lfs_pointer(r#"{"n":4,"indptr":[0],"indices":[]}"#));
        assert!(!is_lfs_pointer(""));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin ssi-ordering-challenge detects_git_lfs_pointer_text 2>&1 | tail -10`
Expected: FAIL — `cannot find function is_lfs_pointer`.

- [ ] **Step 3: Implement the detector and wire it in**

Add to `src/corpus.rs` (module level, above `corpus_indexed`):

```rust
/// A Git LFS pointer file (what a clone without `git-lfs` leaves at a tracked
/// path) begins with this line. Detecting it lets the harness print an
/// actionable message instead of an opaque JSON parse error.
fn is_lfs_pointer(text: &str) -> bool {
    text.trim_start()
        .starts_with("version https://git-lfs.github.com/spec/")
}
```

Then, in `corpus_indexed`, after `let path = corpus_path();` and before the `ssi_scoring::load_corpus_jsonl` call, add a pre-read guard:

```rust
    // If the corpus is an unresolved Git LFS pointer (clone without git-lfs),
    // fail loudly with a fix, not a confusing parse error.
    if let Ok(text) = std::fs::read_to_string(&path) {
        if is_lfs_pointer(&text) {
            panic!(
                "{} is an unresolved Git LFS pointer, not the corpus.\n\
                 Install git-lfs and fetch the real file:\n\
                 \x20   git lfs install && git lfs pull",
                path.display()
            );
        }
    }
```

Note: `corpus_indexed` already reads the file again below via `load_corpus_jsonl` and `read_to_string`; this extra small read happens once at startup and is acceptable (the file is read fully anyway). The `panic!` is consistent with the existing `panic!` on load failure a few lines down.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bin ssi-ordering-challenge detects_git_lfs_pointer_text 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test 2>&1 | tail -15`
Expected: PASS — all tests, including the existing `corpus` tests, unchanged.

- [ ] **Step 6: Commit**

```bash
git add src/corpus.rs
git commit -m "feat(corpus): detect unresolved Git LFS pointer, print git lfs pull hint

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Install Git LFS and add the tracking rule

Set up Git LFS in the repo and track the corpus path — BEFORE committing the large file, so the file is stored as an LFS object from the first commit (never as a 99 MB blob in git history).

**Files:**
- Create: `.gitattributes`

**Interfaces:** none.

- [ ] **Step 1: Install the git-lfs binary**

git-lfs is not installed on this machine (`git lfs version` fails). Install it:

Run: `brew install git-lfs && git lfs version`
Expected: prints a version like `git-lfs/3.x.x`.

(If `brew` is unavailable, this is the one step to hand to the user with `!brew install git-lfs`.)

- [ ] **Step 2: Initialize LFS for this repo and add the tracking rule**

Run: `git lfs install --local && git lfs track "corpus/dev/patterns.jsonl"`
Expected: creates/updates `.gitattributes` with a line like
`corpus/dev/patterns.jsonl filter=lfs diff=lfs merge=lfs -text` and prints `Tracking "corpus/dev/patterns.jsonl"`.

- [ ] **Step 3: Verify the .gitattributes content**

Run: `cat .gitattributes`
Expected: contains exactly (order/spacing may vary):
`corpus/dev/patterns.jsonl filter=lfs diff=lfs merge=lfs -text`

- [ ] **Step 4: Commit the tracking rule (before the big file)**

```bash
git add .gitattributes
git commit -m "build(corpus): track dev corpus with Git LFS

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Replace the sample with the full 300-pattern corpus (as an LFS object)

Copy the real corpus over the sample path and commit it — with `.gitattributes` already in place, it is stored as an LFS object.

**Files:**
- Modify: `corpus/dev/patterns.jsonl` (13 lines → 300 lines, LFS-backed)

**Interfaces:** none.

- [ ] **Step 1: Copy the full corpus over the sample path**

Run: `cp /Users/lulu/Documents/autoresearch/ordering_challenge/corpus-generation/corpus/dev/patterns.jsonl corpus/dev/patterns.jsonl`
Then verify size and bucket split:
Run: `wc -l corpus/dev/patterns.jsonl && du -h corpus/dev/patterns.jsonl && awk -F'"n":' '{split($2,a,","); n=a[1]; if(n<1000)s++; else if(n<10000)m++; else l++} END{print "lt_1k="s" 1k_10k="m" gt_10k="l}' corpus/dev/patterns.jsonl`
Expected: `300` lines, `~99M`, `lt_1k=147 1k_10k=108 gt_10k=45`.

- [ ] **Step 2: Stage and confirm it is going to LFS (not a git blob)**

Run: `git add corpus/dev/patterns.jsonl && git lfs status`
Expected: lists `corpus/dev/patterns.jsonl` under "Objects to be committed" as `LFS`. Double-check:
Run: `git check-attr filter -- corpus/dev/patterns.jsonl`
Expected: `corpus/dev/patterns.jsonl: filter: lfs`.

- [ ] **Step 3: Commit the LFS-backed corpus**

```bash
git commit -m "feat(corpus): ship full 300-pattern dev corpus via Git LFS

Replaces the 13-pattern smoke-test sample with the real dev corpus
(300 patterns, n up to ~340k), stored as a Git LFS object. Populates
all three size buckets (lt_1k=147, 1k_10k=108, gt_10k=45), so the
bucketed weighted score is exercised locally.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: Verify the committed object is a pointer in the tree, bytes in LFS**

Run: `git cat-file -p HEAD:corpus/dev/patterns.jsonl | head -3`
Expected: the LFS pointer text (`version https://git-lfs.github.com/spec/v1`, `oid sha256:...`, `size 103879806`) — NOT JSONL. This confirms the git tree holds the pointer, not the 99 MB blob.

Run: `git lfs ls-files | grep patterns.jsonl`
Expected: one line with the oid and `corpus/dev/patterns.jsonl`.

- [ ] **Step 5: Verify the working-tree file is the real corpus and the harness runs on it**

Run: `head -c 80 corpus/dev/patterns.jsonl`
Expected: JSONL (`{"n":...`), not a pointer (LFS smudged it in the working tree).

Run: `cargo run --release 2>&1 | tail -12`
Expected: per-bucket table with all three buckets populated (`lt_1k=147, 1k_10k=108, gt_10k=45`), a real weighted score, and `score.json` written. This is slower than the sample (seconds–minutes) — that is the intended default now.

Re-baseline check — the shipped starter calls `feral_amd::amd_order` (same as the baseline), so it should still tie:
Run: `python3 -c "import json; d=json.load(open('score.json')); print('score', d['score'], 'matrices', d['metrics']['matrices'])"`
Expected: `score` ≈ `1.0`, `matrices 300`. (If it is not ~1.0, investigate before proceeding — the starter should tie AMD.)

---

## Task 7: Update docs to match the new score + corpus reality

Rewrite the score description everywhere it appears, and replace the dead "download from GitHub release" corpus sections with the Git LFS story. Also add `lfs: true` to the CI checkout and reconcile `results.tsv`.

**Files:**
- Modify: `.github/workflows/benchmark.yml:33-34`
- Modify: `README.md` (score block ~68-76; corpus section 115-148; the agent-quickstart note 184-192)
- Modify: `RULES.md:14-18`
- Modify: `corpus/dev/README.md` (whole file)
- Modify: `benchmark.json` (`description`)
- Modify: `results.tsv` (header comment + starter row note)

**Interfaces:** none.

- [ ] **Step 1: Add `lfs: true` to the CI checkout step**

In `.github/workflows/benchmark.yml`, replace:

```yaml
      - name: Checkout dispatched ref
        uses: actions/checkout@v4 # checks out the dispatched ref by default
```

with:

```yaml
      - name: Checkout dispatched ref
        uses: actions/checkout@v4 # checks out the dispatched ref by default
        with:
          # The dev corpus is a Git LFS object. When no eval bucket is
          # configured, the self-grade reads the committed dev corpus, so the
          # runner must fetch LFS bytes (default is lfs: false → pointer file).
          # When an eval bucket IS configured, grading uses SSI_CORPUS_FILE and
          # never reads this file; fetching it is a small, harmless cost.
          lfs: true
```

- [ ] **Step 2: Rewrite the README score block**

In `README.md`, replace the block at lines 68-76 (from `5. **Scores** the run as` through `appended to \`results.tsv\`.`) with:

```markdown
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
```

- [ ] **Step 3: Rewrite the README corpus section (lines 115-148)**

Replace the whole `### The corpus: in-repo sample vs. full download` section (lines 115-148) with:

```markdown
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
```

- [ ] **Step 4: Rewrite the README agent-quickstart note (lines 184-192)**

Replace the blockquote `> **Before you start: download the full dev corpus.** ...` (lines 184-192) with:

```markdown
> **Before you start: make sure Git LFS is installed** (`git lfs install`), so
> `corpus/dev/patterns.jsonl` resolves to the real 300-pattern corpus rather
> than an LFS pointer. If you cloned without it, run `git lfs pull`. See
> "The corpus" above and `corpus/dev/README.md`.
```

- [ ] **Step 5: Rewrite the RULES.md goal block (lines 14-18)**

Replace lines 14-18 with:

```markdown
Minimize the harness score: the **weighted mean over size buckets of the
within-bucket geomean flop ratio** versus the AMD baseline (buckets lt_1k /
1k_10k / gt_10k by dimension n, weights 0.30 / 0.30 / 0.40; the AMD baseline is
anchored at 1.00). Lower is better; beating AMD means a score < 1.00. Read your
own score and per-bucket breakdown from `score.json` / `results.tsv` after a run
— do not assume a reference number, because the corpus is rebaselined per round
and absolute values shift.
```

- [ ] **Step 6: Rewrite corpus/dev/README.md**

Replace the whole file with:

```markdown
# Development corpus

`patterns.jsonl` here is the **full development corpus** (300 patterns, n up to
~340,000), one CSC sparsity pattern per line. It spans the families NLP / QCP /
QP / QCQP and populates all three scoring size buckets (lt_1k / 1k_10k / gt_10k).

It is shipped in-repo via **Git LFS** because the file is ~99 MB. Install Git LFS
before cloning, or fetch it afterward:

```sh
git lfs install
git lfs pull   # if you cloned before installing git-lfs
```

Without `git-lfs`, this path holds a small text *pointer* instead of JSONL, and
the harness stops with a message telling you to run `git lfs pull`.

## Line format

Each line is one symmetric sparsity pattern as compressed-sparse-column (CSC):

```json
{"n": 4, "nnz": 12, "indptr": [...], "indices": [...], "hash": "...", "source": "st_e09"}
```

- `n` — matrix dimension. `indptr` (len n+1) / `indices` (len nnz) — CSC columns.
- The stored pattern is the **full symmetrized** pattern and **includes the
  diagonal**; the harness reader (`ssi_scoring::pattern_from_jsonl_line`) drops
  the diagonal to produce the off-diagonal contract `Pattern`.
- `hash` (SHA-256 of the canonical pattern) and `source` (origin problem) are
  metadata; the harness uses `source` as the display name.

## Grading a different corpus

Point the harness at another corpus for one run with `SSI_CORPUS_FILE`:

```sh
SSI_CORPUS_FILE=/path/to/other.jsonl cargo run --release
```

Unset, the harness grades this in-repo corpus. The competition's hidden
evaluation corpus is never published.
```

- [ ] **Step 7: Update benchmark.json description**

In `benchmark.json`, replace the `description` value:

```json
  "description": "Fill-reducing elimination ordering for sparse symmetric indefinite matrices; scored as a size-bucketed weighted geomean of predicted factorization flops vs feral AMD (lower is better).",
```

- [ ] **Step 8: Reconcile results.tsv header + starter row**

In `results.tsv`, replace the header comment block and starter row. The current header describes "geomean flop ratio"; update to the bucketed definition, and update the starter note. Replace:

```
# and your --note. The row below is the shipped feral-amd starter on the in-repo
# 13-matrix sample — it ties the AMD baseline at 1.00 as a correct starting point.
1782160339	OK	1.000000	1.000000	starter: feral-amd ordering on dev sample (ties AMD baseline)
```

with (keep the timestamp; update text):

```
# and your --note. The row below is the shipped feral-amd starter on the full
# 300-pattern dev corpus — it ties the AMD baseline at 1.00 as a correct start.
1782160339	OK	1.000000	1.000000	starter: feral-amd ordering on full dev corpus (ties AMD baseline)
```

Also update the definition lines just above (the `# ... score (geomean flop ratio vs feral-AMD,` comment) to:

```
# Unix timestamp, status (OK/FAIL), score (weighted mean of per-bucket geomean
# flop ratios vs feral-AMD, lower is better, AMD = 1.00), fill_ratio (same
# weighted scheme over nnz(L), the tiebreak), and your --note.
```

- [ ] **Step 9: Verify docs build/consistency and nothing references the dead release**

Run: `grep -rn "releases/latest/download\|GitHub release\|279 pattern\|13-matrix\|13 matrices\|geomean over" README.md RULES.md corpus/dev/README.md benchmark.json results.tsv`
Expected: no matches (all dead-release and old-score references removed).

Run: `cargo build --release 2>&1 | tail -3`
Expected: builds clean.

- [ ] **Step 10: Commit**

```bash
git add .github/workflows/benchmark.yml README.md RULES.md corpus/dev/README.md benchmark.json results.tsv
git commit -m "docs+ci: bucketed weighted score, Git LFS corpus, lfs:true on checkout

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Final full-suite verification

Confirm the whole change is green and the end-to-end run is correct on the new default corpus.

**Files:** none (verification only).

**Interfaces:** none.

- [ ] **Step 1: Full test suite**

Run: `cargo test 2>&1 | tail -30`
Expected: PASS — `scoring_tests` (7), `detects_git_lfs_pointer_text`, all `ssi-scoring` closed-form tests, `scorer_crosscheck`, `corpus` tests, `time_cap`.

- [ ] **Step 2: End-to-end run on the LFS corpus**

Run: `cargo run --release 2>&1 | tail -15`
Expected: per-bucket table (all three buckets populated), weighted score ≈ 1.0 for the feral-amd starter, `score.json` + `results.tsv` row written.

- [ ] **Step 3: Confirm the git tree is clean and the corpus is an LFS object**

Run: `git status && git lfs ls-files`
Expected: clean tree (score.json is git-ignored); `git lfs ls-files` lists `corpus/dev/patterns.jsonl`.

- [ ] **Step 4: Note on pushing (LFS upload)**

When the branch is pushed, `git push` uploads the LFS object to the remote's LFS store automatically (requires LFS enabled on the GitHub repo). No extra command is needed, but the first push transfers ~99 MB. This is a push-time action — do NOT push unless the user asks.

---

## Self-Review

**Spec coverage:**
- Bucketed score definition (bounds/weights/renormalize) → Tasks 1, 2. ✓
- Tiebreak uses same scheme → Task 2 (Step 3, `fill` via `combine`). ✓
- score.json extends `metrics`, all 3 buckets always listed, empty → null → Task 2 (Steps 3, 5). ✓
- results.tsv byte-identical layout, header/starter reconciled → Task 7 (Step 8). ✓
- main.rs docstring, README, RULES, benchmark.json → Tasks 3, 7. ✓
- size_bucket + combine unit tests incl. worked example + empty-bucket cases → Task 1. ✓
- LFS: .gitattributes, full corpus at default path, 300/99MB → Tasks 5, 6. ✓
- LFS pointer detection test + guard → Task 4. ✓
- benchmark.yml lfs:true → Task 7 (Step 1). ✓
- corpus/dev/README LFS story, dead release links removed → Task 7 (Steps 3, 6, 9). ✓
- Invariant 4 closed-form tests untouched → verified Tasks 2, 8. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step shows full code. ✓

**Type consistency:** `size_bucket`, `geomean`, `combine`, `BucketAcc`, `BUCKETS`, `BUCKET_KEYS`, `BUCKET_WEIGHTS`, `is_lfs_pointer` used with identical signatures across Tasks 1, 2, 4. `combine` takes `&[Option<f64>; 3]` in both definition (Task 1) and call (Task 2). ✓

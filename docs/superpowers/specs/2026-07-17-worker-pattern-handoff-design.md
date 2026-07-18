# Worker pattern handoff — fix O(corpus) re-read per invocation (issue #20)

## Problem

Each capped worker invocation re-derives its one matrix from the raw corpus
bytes, and the parent reads the whole corpus file twice per run. Two distinct
redundancies:

1. **Parent double-read.** `corpus_indexed()` (`src/corpus.rs`) calls
   `load_corpus_jsonl` (full `read_to_string` + parse of every line), then does
   a *second* `std::fs::read_to_string(&path)` on the identical file solely to
   recover each kept line's raw file-line index — a number that exists only to
   feed the worker.

2. **Worker re-read (the headline).** The parent spawns the worker with
   `--worker <jsonl_path> <line_index> <out_perm>`. With only a path + index,
   the worker calls `ssi_scoring::load_pattern_jsonl_line`, which does
   `read_to_string` of the WHOLE corpus and `lines().nth(line_index)` to scan to
   its one line. This happens **inside `watchdog::run_capped`** (counted against
   the 2 s per-matrix time cap) and **~2× per matrix** (the determinism gate runs
   `order()` twice, runs `a` and `b`). Cost is O(corpus size) per worker
   invocation though the worker needs exactly one matrix.

Root cause: the handoff format. The parent parses every `Pattern` at startup,
then throws them away at the process boundary and hands the worker a path+index,
forcing re-derivation. Redundancy 1 exists only to make that path+index work.

On today's 99 MB / 300-pattern dev corpus the re-read overhead is ~30 ms (well
under the 2 s cap) — not a current correctness/timeout risk. The concern is
scalability: the hidden eval corpus is stated to be larger, and the grader runs
this same binary ~3–5× slower, so on a ~1 GB corpus each worker's full read
could consume a meaningful fraction of the cap. It also wastes work regardless.

## Approach (chosen: A — pass the parsed pattern)

The parent hands the worker its already-parsed `Pattern` instead of a corpus
path + index. This fixes both redundancies at once, is a net simplification
(more lines deleted than added), collapses to a single JSONL parser, and tightens
Invariant 2 (the worker's pattern is byte-identical to what the parent scored).

Rejected alternatives:
- **B (byte-offset seek):** worker seeks to a precomputed offset and parses one
  line. O(line size), but keeps two parsers and the corpus-file dependency in
  the worker — more moving parts for the same win.
- **C (parent double-read only):** removes redundancy 1, leaves the worker's
  O(corpus) re-read. Partial fix.

## Design

### New module `src/pattern_io.rs` (trusted harness, stdlib-only)

A binary serializer for `Pattern`, mirroring the existing `perm_io` wire format
and living in the harness (NOT in `ssi-scoring`, NOT in the submission dir —
Invariant 3 preserved).

Wire format (all little-endian u64):

```
n | col_ptr_len | col_ptr[0..col_ptr_len] | row_idx_len | row_idx[0..row_idx_len]
```

- `write_pattern(path: &Path, pat: &Pattern) -> io::Result<()>`
- `read_pattern(path: &Path) -> io::Result<Pattern>`

`read_pattern` reconstructs directly from the public `Pattern` fields (`n`,
`col_ptr`, `row_idx` are all `pub`). It does NOT re-run `Pattern::from_adjacency`
— it faithfully round-trips the exact bytes the parent already validated, so the
worker's pattern is byte-identical to the one the parent scored. It applies the
same defensive guards `perm_io::read_perm` uses (minimum length, per-length
`checked_mul`/`checked_add` overflow checks against the actual byte count) plus a
cheap structural sanity check: `col_ptr.len() == n + 1`, `col_ptr[0] == 0`,
`col_ptr.last() == row_idx.len()`. A malformed file is an `io::Error`, never a
panic — matching `perm_io`.

### `src/main.rs`

- Worker CLI becomes `--worker <pattern_file> <out_perm>`. `worker()` parses two
  positional args, calls `pattern_io::read_pattern(pattern_file)` instead of
  `ssi_scoring::load_pattern_jsonl_line`, then runs `order()` and writes the perm
  exactly as today. Corpus path + line-index args removed; the `jsonl_path`
  binding and the "worker must load from the SAME corpus file" comment go away.
- In the per-matrix loop, before the two `run_once` calls, serialize the pattern
  **once** to `scratch/<seq>-pat.bin` (`pattern_io::write_pattern`). Both the `a`
  and `b` runs pass that one file (one-per-matrix, shared — chosen). The parent
  write happens OUTSIDE the timed window.
- The scratch-file stem no longer needs to be a raw file-line index; a plain
  enumeration counter suffices (see corpus.rs below).

### `src/corpus.rs`

- `corpus_indexed()` collapses: drop the second `read_to_string`, the
  `raw_indices` recovery, and the `zip`. Since it no longer tracks raw indices,
  rename it (e.g. `dev_corpus()` / `load_corpus()`) and return
  `Vec<(String, Pattern)>` — a thin wrapper over `load_corpus_jsonl` that keeps
  the LFS-pointer guard and the empty-on-missing behavior. The call site in
  `main.rs` uses `.enumerate()` to get the scratch-file counter.
- Keep the Git-LFS-pointer prefix check and the "empty vec when file absent"
  behavior — those are unrelated to the re-read and still wanted.

### `ssi-scoring/src/loader.rs` + `lib.rs`

- **Delete** `load_pattern_jsonl_line` and its `pub` re-export in `lib.rs`.
  Nothing else uses it (the grader runs this same binary; there is no separate
  grader worker), and it carries a live footgun: its own WARNING that its
  blank-line index space diverges from `load_corpus_jsonl`.
- Keep `pattern_from_jsonl_line` and `load_corpus_jsonl` — still the single JSONL
  parser (Invariant 2 at the parsing boundary).

## Tests (TDD — write first, watch fail, then implement)

- **`pattern_io` round-trip:** a representative `Pattern` (e.g. the n=4 corpus
  line) survives `write_pattern` → `read_pattern` with identical `n`, `col_ptr`,
  `row_idx`. Reject a truncated file and an overflow-length header (mirrors
  `perm_io`'s truncated/overflow tests). Reject a structurally inconsistent file
  (e.g. `col_ptr.last() != row_idx.len()`).
- **`corpus.rs`:** replace `indexed_corpus_matches_single_line_loader` (which
  cross-checked the deleted single-line loader) with a test asserting
  `corpus_indexed()` yields the same patterns, in the same order, as
  `load_corpus_jsonl` on the dev corpus.
- **`loader.rs`:** remove `load_corpus_and_single_line_agree`'s single-line
  assertions (or the whole test if it only exercised the deleted function); keep
  the parse/round-trip coverage of `load_corpus_jsonl`.
- **Regression (must stay green):** the closed-form scorer tests (Invariant 4)
  and `tests/time_cap.rs` (end-to-end spawn of the real worker under the cap).

## Net effect

- Worker read: O(corpus) → O(one pattern). Parent: two full reads → one.
- One JSONL parser; the parent↔worker handoff is an internal binary format.
- No contract change: `order()` signature, score definition, gates, and
  `score.json`/`results.tsv` formats are all untouched (Invariant 1). The wire
  format between parent and worker is not part of the contract.
- Invariant 2 tightened: the worker scores the byte-identical pattern the parent
  parsed, rather than re-parsing from raw bytes.

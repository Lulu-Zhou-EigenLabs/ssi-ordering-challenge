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

/// Load the dev corpus tagged with each entry's 0-based RAW line index in
/// `patterns.jsonl` (blank lines counted), so the parent can hand the worker the
/// exact index `ssi_scoring::load_pattern_jsonl_line` will resolve. Parsing
/// still goes through the shared `ssi_scoring::load_corpus_jsonl` reader; this
/// only recovers the raw indices of the non-blank lines it kept.
pub fn dev_corpus_indexed() -> Vec<(usize, String, Pattern)> {
    let path = PathBuf::from(DEV_CORPUS_FILE);
    let corpus = match ssi_scoring::load_corpus_jsonl(&path) {
        Ok(c) => c,
        Err(e) => {
            if path.exists() {
                panic!("failed to load {}: {e}", path.display());
            }
            return Vec::new();
        }
    };
    // Recover the raw line index of each non-blank line, in file order — the
    // same lines load_corpus_jsonl parsed, in the same order.
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let raw_indices: Vec<usize> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, _)| i)
        .collect();
    debug_assert_eq!(raw_indices.len(), corpus.len(), "index/corpus length mismatch");
    corpus
        .into_iter()
        .zip(raw_indices)
        .map(|((source, pat), idx)| (idx, source, pat))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_corpus_matches_single_line_loader() {
        // Each (raw_index, _, pattern) from dev_corpus_indexed must equal the
        // pattern load_pattern_jsonl_line resolves for that raw index — proving
        // the parent and worker agree on which matrix an index names.
        let path = std::path::Path::new(DEV_CORPUS_FILE);
        for (idx, _src, pat) in dev_corpus_indexed() {
            let one = ssi_scoring::load_pattern_jsonl_line(path, idx)
                .expect("worker loader resolves the raw index");
            assert_eq!(one.n, pat.n, "n mismatch at raw line {idx}");
            assert_eq!(one.col_ptr, pat.col_ptr, "col_ptr mismatch at raw line {idx}");
            assert_eq!(one.row_idx, pat.row_idx, "row_idx mismatch at raw line {idx}");
        }
    }
}

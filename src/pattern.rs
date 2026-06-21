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

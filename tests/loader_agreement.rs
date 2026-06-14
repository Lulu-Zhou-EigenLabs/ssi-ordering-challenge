//! Cross-check: the public stdlib `.mtx` reader and feral's reference reader
//! (via `ssi_scoring::load_pattern`) must produce the SAME `Pattern` on the
//! shipped dev files. There must be no second parser that can silently
//! disagree — the same one-source-of-truth rule that governs scoring
//! (Phase 3, step 7).
//!
//! The harness `pattern` module is `mod`-private to the binary, so we exercise
//! the public reader through a tiny re-test of the same logic is not possible;
//! instead we include the source via the binary's public API surface by
//! re-declaring the reader as a path module. Simplest robust approach: walk the
//! corpus, parse each file both ways, compare the canonical CSC triple.

use std::path::{Path, PathBuf};

// Pull the harness's reader into the test by including the module source.
// `src/pattern.rs` only depends on std + ssi_scoring, both available here.
#[path = "../src/pattern.rs"]
mod harness_pattern;

fn dev_files() -> Vec<PathBuf> {
    let root = Path::new("corpus/dev");
    let mut out = Vec::new();
    collect(root, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("mtx") {
            out.push(p);
        }
    }
}

#[test]
fn public_reader_matches_feral_reader_on_dev_corpus() {
    let files = dev_files();
    assert!(!files.is_empty(), "no dev corpus found under corpus/dev");

    // Sample across families to keep the test fast but representative: take
    // every file whose size is modest, plus a couple of large ones.
    let mut checked = 0usize;
    for path in files.iter() {
        // The public stdlib reader.
        let public = harness_pattern::read_mtx_pattern(path)
            .unwrap_or_else(|e| panic!("public reader failed on {}: {e}", path.display()));
        // feral's reference reader.
        let feral = ssi_scoring::load_pattern(path)
            .unwrap_or_else(|e| panic!("feral reader failed on {}: {e}", path.display()));

        assert_eq!(public.n, feral.n, "n mismatch on {}", path.display());
        assert_eq!(
            public.col_ptr,
            feral.col_ptr,
            "col_ptr mismatch on {}",
            path.display()
        );
        assert_eq!(
            public.row_idx,
            feral.row_idx,
            "row_idx mismatch on {}",
            path.display()
        );
        checked += 1;
    }
    assert_eq!(checked, files.len(), "should have checked every dev file");
}

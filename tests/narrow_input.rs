//! NARROW INPUT (proposal §3.1) — preserved as a hard property and asserted.
//!
//! `order()` receives ONLY the sparsity pattern: no values, no right-hand side,
//! no answer. The repo's `Pattern` type carries structure only, by
//! construction. This test pins that: two matrices with the SAME pattern but
//! (conceptually) different values are indistinguishable to the scorer, because
//! the loader drops values (Phase 1 R6 / Phase 2 D3) and `Pattern` has no field
//! to carry them.
//!
//! We demonstrate value-independence directly: feral's `.mtx` reader requires a
//! value column, and our loader ignores it — so two files identical in
//! structure but differing in every value load to byte-identical `Pattern`s and
//! score identically.

fn write_tmp(name: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("ssi-narrow-input-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn values_do_not_affect_the_loaded_pattern_or_score() {
    // Same 3×3 structure (a path 1-2-3 plus diagonal), different values.
    let a = "%%MatrixMarket matrix coordinate real symmetric\n\
             3 3 5\n\
             1 1 1.0\n2 2 1.0\n3 3 1.0\n2 1 2.0\n3 2 2.0\n";
    let b = "%%MatrixMarket matrix coordinate real symmetric\n\
             3 3 5\n\
             1 1 -9.5\n2 2 1e6\n3 3 0.001\n2 1 -42.0\n3 2 7.7\n";

    let pa = ssi_scoring::load_pattern(&write_tmp("a.mtx", a)).unwrap();
    let pb = ssi_scoring::load_pattern(&write_tmp("b.mtx", b)).unwrap();

    // Byte-identical patterns: the values were never consulted.
    assert_eq!(pa.n, pb.n);
    assert_eq!(pa.col_ptr, pb.col_ptr);
    assert_eq!(pa.row_idx, pb.row_idx);

    // And therefore identical scores under any ordering.
    let id: Vec<usize> = (0..pa.n).collect();
    assert_eq!(ssi_scoring::score(&pa, &id), ssi_scoring::score(&pb, &id));
}

// The former `loaded_dev_file_score_is_value_independent` test mangled the
// value column of a committed `.mtx` dev file and asserted the score was
// unchanged. The dev corpus is now `corpus/dev/patterns.jsonl`, which is
// pattern-only BY CONSTRUCTION — it carries no values at all, so there is no
// value column to mangle and NARROW INPUT holds structurally rather than by
// demonstration. The value-independence of the `.mtx` `load_pattern` path (used
// by the grader's corpus tooling) remains pinned by the hand-written test
// above.

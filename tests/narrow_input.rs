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

use std::path::Path;

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

#[test]
fn loaded_dev_file_score_is_value_independent() {
    // Load a real committed dev file, then a copy with every value replaced by a
    // distinct sentinel; the scores must match.
    let orig = Path::new("corpus/dev/ampl/ampl_tutorial_flow_density__iter0.mtx");
    let text = std::fs::read_to_string(orig).unwrap();
    // Rewrite every entry's value column to a wildly different number.
    let mut out = String::new();
    let mut past_size = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('%') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if !past_size {
            out.push_str(line);
            out.push('\n');
            past_size = true;
            continue;
        }
        let mut it = t.split_whitespace();
        let (r, c) = (it.next().unwrap(), it.next().unwrap());
        out.push_str(&format!("{r} {c} 123456.789\n"));
    }
    let mangled = write_tmp("mangled.mtx", &out);

    let p_orig = ssi_scoring::load_pattern(orig).unwrap();
    let p_mangled = ssi_scoring::load_pattern(&mangled).unwrap();
    assert_eq!(p_orig.col_ptr, p_mangled.col_ptr);
    assert_eq!(p_orig.row_idx, p_mangled.row_idx);
    let id: Vec<usize> = (0..p_orig.n).collect();
    assert_eq!(
        ssi_scoring::score(&p_orig, &id),
        ssi_scoring::score(&p_mangled, &id)
    );
}

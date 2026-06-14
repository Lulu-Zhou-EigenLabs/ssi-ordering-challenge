//! Sparsity patterns, the development corpus loader, and the public-side
//! Matrix Market reader.
//!
//! HARNESS FILE — do not modify. The `Pattern` type a contestant ordering sees
//! is `ssi_scoring::Pattern`, re-exported here so the contract signature
//! `order(pattern: &Pattern) -> Vec<usize>` is byte-identical to what the
//! grader scores. It carries structure only — never values, never a
//! right-hand side (NARROW INPUT, proposal §3.1).
//!
//! ## Two readers, proven identical
//!
//! The trusted/grader side parses `.mtx` with feral's reference reader
//! (`ssi_scoring::load_pattern`). This file additionally provides a small
//! stdlib MatrixMarket reader for the public harness. A cross-check test
//! (`tests/loader_agreement.rs`) asserts the two produce the SAME `Pattern` on
//! the shipped dev files, so there is no second parser that can silently
//! disagree (Phase 3, step 7 — same rule as one-scoring-path).

use std::fs;
use std::path::{Path, PathBuf};

pub use ssi_scoring::Pattern;

/// Directory holding the shipped public development corpus (`.mtx` files).
pub const DEV_CORPUS_DIR: &str = "corpus/dev";

/// Read a Matrix Market `.mtx` file into a `Pattern` using a small stdlib
/// reader. Accepts only `%%MatrixMarket matrix coordinate real symmetric`
/// (case-insensitive banner, like feral's reader). Indices convert 1-based →
/// 0-based; values are parsed-past and IGNORED (the score is pattern-only,
/// Phase 1 R6 / Phase 2 D3). The diagonal is dropped; entries are symmetrized.
pub fn read_mtx_pattern(path: &Path) -> Result<Pattern, String> {
    let contents =
        fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    parse_mtx_pattern(&contents)
}

/// Parse Matrix Market content (banner + size line + `row col value` triplets)
/// into a `Pattern`. Separated from file IO so it is unit-testable.
pub fn parse_mtx_pattern(contents: &str) -> Result<Pattern, String> {
    let mut lines = contents.lines();

    // Banner: tokenize case-insensitively (matches feral's tolerant reader).
    let header = lines.next().ok_or("empty file")?;
    const BANNER: [&str; 5] = [
        "%%matrixmarket",
        "matrix",
        "coordinate",
        "real",
        "symmetric",
    ];
    let mut toks = header.split_whitespace();
    let banner_ok = BANNER
        .iter()
        .all(|exp| toks.next().is_some_and(|t| t.eq_ignore_ascii_case(exp)))
        && toks.next().is_none();
    if !banner_ok {
        return Err(format!(
            "unsupported header '{}' (expected: %%MatrixMarket matrix coordinate real symmetric)",
            header.trim()
        ));
    }

    // Size line: first non-comment, non-empty line.
    let mut size_line = None;
    for line in lines.by_ref() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('%') {
            continue;
        }
        size_line = Some(t.to_string());
        break;
    }
    let size_text = size_line.ok_or("missing size line")?;
    let parts: Vec<&str> = size_text.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(format!("expected 'rows cols nnz', got '{size_text}'"));
    }
    let n: usize = parts[0].parse().map_err(|_| "invalid row count")?;
    let n_cols: usize = parts[1].parse().map_err(|_| "invalid col count")?;
    if n != n_cols {
        return Err(format!("not square: {n} rows, {n_cols} cols"));
    }

    // Entries: "row col value". 1-based → 0-based. Value token IGNORED.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for line in lines {
        let t = line.trim();
        if t.is_empty() || t.starts_with('%') {
            continue;
        }
        let mut it = t.split_whitespace();
        let r1: usize = it
            .next()
            .ok_or("entry missing row")?
            .parse()
            .map_err(|_| "invalid row index")?;
        let c1: usize = it
            .next()
            .ok_or("entry missing col")?
            .parse()
            .map_err(|_| "invalid col index")?;
        // A value token is required by the format but the score ignores it.
        if r1 < 1 || r1 > n || c1 < 1 || c1 > n {
            return Err(format!("entry ({r1},{c1}) out of range 1..={n}"));
        }
        let (r, c) = (r1 - 1, c1 - 1);
        if r == c {
            continue; // diagonal dropped
        }
        // Symmetrize: store both triangles regardless of which was given.
        adj[c].push(r);
        adj[r].push(c);
    }

    Ok(pattern_from_adjacency(n, adj))
}

/// Build a `Pattern` from per-vertex adjacency lists, sorting + deduplicating
/// each column. Uses only `Pattern`'s public fields, so the harness reader
/// needs no privileged access to ssi-scoring internals.
fn pattern_from_adjacency(n: usize, mut adj: Vec<Vec<usize>>) -> Pattern {
    let mut col_ptr = Vec::with_capacity(n + 1);
    let mut row_idx = Vec::new();
    col_ptr.push(0);
    for list in adj.iter_mut() {
        list.sort_unstable();
        list.dedup();
        row_idx.extend_from_slice(list);
        col_ptr.push(row_idx.len());
    }
    Pattern { n, col_ptr, row_idx }
}

/// Load the shipped development corpus: every `.mtx` under `DEV_CORPUS_DIR`,
/// sorted by family/name for a deterministic run order.
///
/// The scored run parses with the public stdlib reader (`read_mtx_pattern`).
/// The grader parses with feral's reference reader (`ssi_scoring::load_pattern`).
/// `tests/loader_agreement.rs` asserts the two produce byte-identical `Pattern`s
/// on every shipped dev file, so the two parsers can never silently disagree —
/// exact-grader equivalence is preserved at the parsing boundary too (step 7).
pub fn dev_corpus() -> Vec<(String, Pattern)> {
    let root = PathBuf::from(DEV_CORPUS_DIR);
    let mut files = Vec::new();
    collect_mtx(&root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = corpus_name(&root, &path);
            let pat = read_mtx_pattern(&path)
                .unwrap_or_else(|e| panic!("load {}: {e}", path.display()));
            (name, pat)
        })
        .collect()
}

/// Short display name for a corpus file: `family/stem` (e.g. `bratu/bratu_n2050`).
fn corpus_name(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut s = rel.with_extension("").to_string_lossy().replace('\\', "/");
    // Strip the "__iterN" suffix the corpus generator appends for readability.
    if let Some(pos) = s.find("__iter") {
        s.truncate(pos);
    }
    s
}

fn collect_mtx(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_mtx(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("mtx") {
            out.push(path);
        }
    }
}

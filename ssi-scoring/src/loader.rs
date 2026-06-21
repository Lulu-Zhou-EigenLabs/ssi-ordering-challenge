//! Matrix Market loader for the trusted/grader side.
//!
//! `load_pattern` is the SCORING WRAPPER's loader: it reuses feral's own
//! `read_mtx` → `to_csc()` → `symmetric_pattern()` (Phase 1 §4, step 7), so the
//! grader parses corpus files with feral's reference parser — the source of
//! truth. The public harness has a *separate* stdlib reader
//! (`crate::pattern`-free, in the harness), and a cross-check test asserts the
//! two produce the SAME `Pattern` so there is no second parser that can
//! disagree (same rule as scoring, step 7).
//!
//! VALUES ARE IGNORED. feral's `read_mtx` requires a value column (Phase 1 R6;
//! Phase 2 D3 writes a dummy `1` per entry), but the score uses only the
//! pattern: we take `symmetric_pattern()` (structure) and drop `values`
//! entirely. The diagonal is stripped to match the contract's `Pattern`
//! (off-diagonal both-triangle storage).

use crate::pattern::Pattern;
use std::fmt;
use std::path::Path;

/// Failure to load a `.mtx` file or a JSONL corpus line into a `Pattern`.
#[derive(Debug)]
pub enum LoadError {
    /// feral's reader rejected the file (bad banner, missing size line, …).
    Read(String),
    /// A `patterns.jsonl` line was malformed or structurally invalid.
    Json(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(m) => write!(f, "failed to read .mtx: {m}"),
            LoadError::Json(m) => write!(f, "failed to parse JSONL pattern: {m}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Load a Matrix Market `.mtx` file into a full-symmetric `Pattern`, ignoring
/// the numeric values. Uses feral's reference reader (the trusted path).
pub fn load_pattern(path: &Path) -> Result<Pattern, LoadError> {
    let mtx = feral::read_mtx(path).map_err(|e| LoadError::Read(e.to_string()))?;
    let csc = mtx
        .to_csc()
        .map_err(|e| LoadError::Read(e.to_string()))?;
    // Full symmetric pattern (both triangles), structure only — values dropped.
    let sym = csc.symmetric_pattern();
    Ok(from_feral_symmetric(&sym))
}

/// Convert feral's full-symmetric `CscPattern` (which may carry diagonal
/// entries) into the contract's `Pattern` (off-diagonal only, both triangles).
fn from_feral_symmetric(sym: &feral::sparse::csc::CscPattern) -> Pattern {
    let n = sym.n;
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for j in 0..n {
        for &i in &sym.row_idx[sym.col_ptr[j]..sym.col_ptr[j + 1]] {
            if i != j {
                adj[j].push(i);
            }
        }
    }
    Pattern::from_adjacency(n, &mut adj)
}

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
///
/// WARNING: this index space differs from `load_corpus_jsonl`, which SKIPS
/// blank lines. If a corpus ever contains blank lines, the `k`-th entry from
/// `load_corpus_jsonl` is NOT necessarily line `k` here. A grader pairing the
/// two must enumerate indices the same way both consume them (the shipped
/// corpus has no blank lines, so the spaces coincide today).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_ignores_values_via_symmetric_pattern() {
        // A 2×2 with a single off-diagonal coupling. Whatever the values are,
        // the loaded pattern is determined by structure alone.
        let mtx = "%%MatrixMarket matrix coordinate real symmetric\n\
                   2 2 3\n\
                   1 1 7.0\n\
                   2 2 9.0\n\
                   2 1 -3.5\n";
        let dir = std::env::temp_dir().join("ssi-scoring-loader-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tiny.mtx");
        std::fs::write(&path, mtx).unwrap();

        let p = load_pattern(&path).unwrap();
        assert_eq!(p.n, 2);
        // Off-diagonal both-triangle: column 0 sees row 1, column 1 sees row 0.
        assert_eq!(p.col(0), &[1]);
        assert_eq!(p.col(1), &[0]);
        // Diagonal entries (1 1) and (2 2) are dropped.
        assert_eq!(p.nnz(), 2);
    }

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
}

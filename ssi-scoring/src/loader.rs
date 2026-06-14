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

/// Failure to load a `.mtx` file into a `Pattern`.
#[derive(Debug)]
pub enum LoadError {
    /// feral's reader rejected the file (bad banner, missing size line, …).
    Read(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(m) => write!(f, "failed to read .mtx: {m}"),
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
}

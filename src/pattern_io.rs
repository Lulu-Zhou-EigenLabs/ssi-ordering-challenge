//! Binary Pattern I/O — the parent→worker handoff for the subprocess-enforced
//! time cap. The parent parses the corpus once, then serializes ONE Pattern per
//! matrix to the scratch dir; the worker reads back just that pattern instead of
//! re-reading the whole corpus (issue #20). Trusted harness code (not scoring,
//! not submission code). `std`-only, mirroring perm_io's shape and guards.
//!
//! Format (all little-endian u64):
//!   n | col_ptr_len | col_ptr[..] | row_idx_len | row_idx[..]
//!
//! read_pattern round-trips the exact bytes the parent already validated — it
//! does NOT re-run Pattern::from_adjacency — so the worker scores the
//! byte-identical Pattern the parent parsed (Invariant 2 at the process
//! boundary). Malformed input is an io::Error, never a panic (like perm_io).

use crate::Pattern;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub fn write_pattern(path: &Path, pat: &Pattern) -> std::io::Result<()> {
    let total = 8 + 8 + pat.col_ptr.len() * 8 + 8 + pat.row_idx.len() * 8;
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&(pat.n as u64).to_le_bytes());
    buf.extend_from_slice(&(pat.col_ptr.len() as u64).to_le_bytes());
    for &v in &pat.col_ptr {
        buf.extend_from_slice(&(v as u64).to_le_bytes());
    }
    buf.extend_from_slice(&(pat.row_idx.len() as u64).to_le_bytes());
    for &v in &pat.row_idx {
        buf.extend_from_slice(&(v as u64).to_le_bytes());
    }
    let mut f = fs::File::create(path)?;
    f.write_all(&buf)
}

pub fn read_pattern(path: &Path) -> std::io::Result<Pattern> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.read_to_end(&mut bytes)?;
    let err = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());

    let mut cur = 0usize;
    let read_u64 = |cur: &mut usize| -> std::io::Result<u64> {
        let end = cur.checked_add(8).ok_or_else(|| err("offset overflow"))?;
        if end > bytes.len() {
            return Err(err("truncated pattern file"));
        }
        let v = u64::from_le_bytes(bytes[*cur..end].try_into().unwrap());
        *cur = end;
        Ok(v)
    };

    let n = read_u64(&mut cur)? as usize;

    let col_ptr_len = read_u64(&mut cur)? as usize;
    let mut col_ptr = Vec::with_capacity(col_ptr_len.min(bytes.len() / 8));
    for _ in 0..col_ptr_len {
        col_ptr.push(read_u64(&mut cur)? as usize);
    }

    let row_idx_len = read_u64(&mut cur)? as usize;
    let mut row_idx = Vec::with_capacity(row_idx_len.min(bytes.len() / 8));
    for _ in 0..row_idx_len {
        row_idx.push(read_u64(&mut cur)? as usize);
    }

    if cur != bytes.len() {
        return Err(err("trailing bytes after pattern"));
    }
    // Structural sanity — cheap, and matches Pattern's documented invariants.
    if col_ptr.len().checked_sub(1) != Some(n) {
        return Err(err("col_ptr length != n+1"));
    }
    if col_ptr.first() != Some(&0) {
        return Err(err("col_ptr[0] != 0"));
    }
    if col_ptr.last().copied() != Some(row_idx.len()) {
        return Err(err("col_ptr[n] != row_idx.len()"));
    }
    Ok(Pattern { n, col_ptr, row_idx })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssi_scoring::pattern_from_jsonl_line;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("ssi-harness-pattern-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn roundtrip_pattern() {
        // A real n=4 corpus line (diagonal dropped by the parser).
        let line = r#"{"n":4,"nnz":12,"indptr":[0,3,6,8,12],"indices":[0,1,3,0,1,3,2,3,0,1,2,3],"hash":"x","source":"m"}"#;
        let (_src, pat) = pattern_from_jsonl_line(line).unwrap();
        let path = tmp("roundtrip.bin");
        write_pattern(&path, &pat).unwrap();
        let back = read_pattern(&path).unwrap();
        assert_eq!(back.n, pat.n);
        assert_eq!(back.col_ptr, pat.col_ptr);
        assert_eq!(back.row_idx, pat.row_idx);
    }

    #[test]
    fn roundtrip_empty_pattern() {
        // n=1, no off-diagonal entries: col_ptr=[0,0], row_idx=[].
        let pat = Pattern { n: 1, col_ptr: vec![0, 0], row_idx: vec![] };
        let path = tmp("empty.bin");
        write_pattern(&path, &pat).unwrap();
        let back = read_pattern(&path).unwrap();
        assert_eq!(back.n, 1);
        assert_eq!(back.col_ptr, vec![0, 0]);
        assert!(back.row_idx.is_empty());
    }

    #[test]
    fn read_rejects_truncated() {
        let path = tmp("trunc.bin");
        std::fs::write(&path, [1u8, 2, 3]).unwrap(); // < 8 bytes
        assert!(read_pattern(&path).is_err());
    }

    #[test]
    fn read_rejects_overflow_len() {
        // n=1, then a col_ptr_len of u64::MAX — must not attempt a huge alloc.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&u64::MAX.to_le_bytes());
        let path = tmp("overflow.bin");
        std::fs::write(&path, bytes).unwrap();
        assert!(read_pattern(&path).is_err());
    }

    #[test]
    fn read_rejects_structural_inconsistency() {
        // Well-formed length prefixes, but col_ptr.last() != row_idx.len().
        let pat = Pattern { n: 2, col_ptr: vec![0, 1, 2], row_idx: vec![1, 0] };
        let path = tmp("bad-struct.bin");
        write_pattern(&path, &pat).unwrap();
        // Corrupt row_idx_len to 1 by rewriting the bytes: easier to hand-build.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u64.to_le_bytes()); // n
        bytes.extend_from_slice(&3u64.to_le_bytes()); // col_ptr_len
        for v in [0u64, 1, 2] { bytes.extend_from_slice(&v.to_le_bytes()); }
        bytes.extend_from_slice(&1u64.to_le_bytes()); // row_idx_len = 1 (inconsistent: col_ptr says 2)
        bytes.extend_from_slice(&0u64.to_le_bytes()); // one row idx
        std::fs::write(&path, bytes).unwrap();
        assert!(read_pattern(&path).is_err());
    }

    #[test]
    fn read_rejects_n_plus_one_overflow() {
        // n=usize::MAX with small col_ptr_len — the n+1 check must not overflow.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(usize::MAX as u64).to_le_bytes()); // n
        bytes.extend_from_slice(&1u64.to_le_bytes()); // col_ptr_len
        bytes.extend_from_slice(&0u64.to_le_bytes()); // col_ptr[0]
        bytes.extend_from_slice(&0u64.to_le_bytes()); // row_idx_len
        let path = tmp("n-overflow.bin");
        std::fs::write(&path, bytes).unwrap();
        assert!(read_pattern(&path).is_err());
    }
}

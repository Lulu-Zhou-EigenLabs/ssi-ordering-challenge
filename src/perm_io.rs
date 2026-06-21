//! Binary permutation I/O — the parent↔worker exchange format for the
//! subprocess-enforced time cap. Ported from the grader's perm_io so the local
//! harness uses the same wire format. Trusted harness code (not scoring, not
//! submission code). `std`-only.
//! Format: little-endian u64 count, then `count` little-endian u64 indices.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub fn write_perm(path: &Path, perm: &[usize]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(8 + perm.len() * 8);
    buf.extend_from_slice(&(perm.len() as u64).to_le_bytes());
    for &p in perm {
        buf.extend_from_slice(&(p as u64).to_le_bytes());
    }
    let mut f = fs::File::create(path)?;
    f.write_all(&buf)
}

pub fn read_perm(path: &Path) -> std::io::Result<Vec<usize>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.len() < 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "perm file too short",
        ));
    }
    let count = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let expected = count.checked_mul(8).and_then(|m| m.checked_add(8));
    if expected != Some(bytes.len()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("perm file length mismatch: header says {count}"),
        ));
    }
    let mut perm = Vec::with_capacity(count);
    for i in 0..count {
        let off = 8 + i * 8;
        perm.push(u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap()) as usize);
    }
    Ok(perm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_perm() {
        let dir = std::env::temp_dir().join("ssi-harness-perm-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.bin");
        let perm = vec![3usize, 1, 0, 2];
        write_perm(&path, &perm).unwrap();
        assert_eq!(read_perm(&path).unwrap(), perm);
    }

    #[test]
    fn read_rejects_truncated() {
        let dir = std::env::temp_dir().join("ssi-harness-perm-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.bin");
        std::fs::write(&path, [1u8, 2, 3]).unwrap(); // < 8 bytes
        assert!(read_perm(&path).is_err());
    }

    #[test]
    fn read_rejects_overflow_count() {
        let dir = std::env::temp_dir().join("ssi-harness-perm-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("overflow.bin");
        std::fs::write(&path, u64::MAX.to_le_bytes()).unwrap();
        assert!(read_perm(&path).is_err());
    }
}

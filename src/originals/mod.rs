//! Reads files from a local vertix-archive clone, checking each against the
//! archive's recorded SHA-256 (`sha256sums.txt`).
//!
//! Nothing from the archive is part of this repository. The server reads
//! what it needs (the map candidates) at start-up and refuses to start if a
//! file is missing, still a Git LFS pointer, or altered.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum Error {
    Io(String),
    Manifest(String),
    /// A file is a Git LFS pointer: the archive clone lacks its content.
    LfsPointer(String),
    HashMismatch {
        what: String,
        expected: String,
        actual: String,
    },
}

impl Error {
    fn io(path: &Path, e: &std::io::Error) -> Self {
        Self::Io(format!("{}: {e}", path.display()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(m) | Self::Manifest(m) => f.write_str(m),
            Self::LfsPointer(p) => write!(
                f,
                "{p} is a Git LFS pointer; run `git lfs pull` in the archive clone"
            ),
            Self::HashMismatch {
                what,
                expected,
                actual,
            } => write!(f, "{what}: SHA-256 {actual}, expected {expected}"),
        }
    }
}

impl std::error::Error for Error {}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn check(what: &str, bytes: &[u8], expected: &str) -> Result<(), Error> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(Error::HashMismatch {
            what: what.to_owned(),
            expected: expected.to_owned(),
            actual,
        })
    }
}

/// A vertix-archive clone, with `sha256sums.txt` loaded so every file
/// read through it can be checked.
pub struct Archive {
    root: PathBuf,
    sums: HashMap<String, String>,
}

impl Archive {
    /// Opens an archive clone and reads its checksum list.
    ///
    /// # Errors
    /// Fails if `sha256sums.txt` cannot be read.
    pub fn open(root: &Path) -> Result<Self, Error> {
        let sums_path = root.join("vertix-preservation/manifests/sha256sums.txt");
        let text = fs::read_to_string(&sums_path).map_err(|e| Error::io(&sums_path, &e))?;
        let sums = text
            .lines()
            .filter_map(|l| {
                let (hash, path) = l.split_once("  ")?;
                Some((path.trim().to_owned(), hash.trim().to_ascii_lowercase()))
            })
            .collect();
        Ok(Self {
            root: root.to_owned(),
            sums,
        })
    }

    /// Reads a file listed in `sha256sums.txt` and checks its hash.
    ///
    /// # Errors
    /// Fails if the file is missing, unlisted, an LFS pointer, or altered.
    pub fn verified(&self, rel: &str) -> Result<Vec<u8>, Error> {
        let path = self.root.join(rel);
        let bytes = fs::read(&path).map_err(|e| Error::io(&path, &e))?;
        if bytes.starts_with(b"version https://git-lfs.github.com/spec/") {
            return Err(Error::LfsPointer(rel.to_owned()));
        }
        let expected = self
            .sums
            .get(rel)
            .ok_or_else(|| Error::Manifest(format!("{rel} is not in sha256sums.txt")))?;
        check(rel, &bytes, expected)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(
            check(
                "x",
                b"abc",
                "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
            )
            .is_ok()
        );
        assert!(matches!(
            check("x", b"abd", "ba78"),
            Err(Error::HashMismatch { .. })
        ));
    }
}

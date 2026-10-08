//! Loads the original client files from a local archive clone.
//!
//! Nothing original is part of this repository. At start-up the server
//! reads every file the boot manifest names from the archive, checks each
//! one against its recorded SHA-256, and refuses to start if any differs.
//! Files inside the Android APK are read from the APK itself, whose own
//! hash is checked against the archive's `sha256sums.txt`.

pub mod page;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The boot manifest as `scripts/import_research.py` writes it.
#[derive(Debug, Deserialize)]
pub struct BootManifest {
    pub build: String,
    pub page_shell: PageShell,
    pub client: Source,
    pub routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
pub struct PageShell {
    pub archive_path: String,
    pub sha256: String,
    pub capture: String,
}

#[derive(Debug, Deserialize)]
pub struct Source {
    pub archive_path: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
pub struct Route {
    pub path: String,
    pub source: String,
    pub archive_path: String,
    pub member: Option<String>,
    pub sha256: String,
    #[serde(default)]
    pub note: String,
}

impl BootManifest {
    /// Parses a manifest file.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = fs::read_to_string(path).map_err(|e| Error::io(path, &e))?;
        serde_json::from_str(&text).map_err(|e| Error::Manifest(format!("{}: {e}", path.display())))
    }
}

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
    Page(String),
}

impl Error {
    fn io(path: &Path, e: &std::io::Error) -> Self {
        Self::Io(format!("{}: {e}", path.display()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(m) | Self::Manifest(m) | Self::Page(m) => f.write_str(m),
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

/// A file ready to serve.
#[derive(Debug, Clone)]
pub struct Asset {
    pub bytes: Arc<[u8]>,
    pub content_type: &'static str,
}

/// Every original the server can serve, verified.
pub struct Store {
    page: Asset,
    by_path: HashMap<String, Asset>,
    /// The APK, for image paths the manifest does not list (hats, camos,
    /// sprays). Its hash is checked once, so its members are trusted.
    apk: Option<Arc<[u8]>>,
    apk_prefix: String,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("routes", &self.by_path.len())
            .field("apk", &self.apk.is_some())
            .finish_non_exhaustive()
    }
}

const APK_WWW: &str = "assets/www/";

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

/// Guesses a Content-Type from a path's extension.
#[must_use]
pub fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript",
        "css" => "text/css",
        "png" => "image/png",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "zip" => "application/zip",
        "txt" => "text/plain; charset=utf-8",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

struct Archive {
    root: PathBuf,
    sums: HashMap<String, String>,
    containers: HashMap<String, Arc<[u8]>>,
}

impl Archive {
    fn open(root: &Path) -> Result<Self, Error> {
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
            containers: HashMap::new(),
        })
    }

    fn read(&self, rel: &str) -> Result<Vec<u8>, Error> {
        let path = self.root.join(rel);
        let bytes = fs::read(&path).map_err(|e| Error::io(&path, &e))?;
        if bytes.starts_with(b"version https://git-lfs.github.com/spec/") {
            return Err(Error::LfsPointer(rel.to_owned()));
        }
        Ok(bytes)
    }

    /// Reads a container (the APK), checked against `sha256sums.txt`.
    fn container(&mut self, rel: &str) -> Result<Arc<[u8]>, Error> {
        if let Some(c) = self.containers.get(rel) {
            return Ok(c.clone());
        }
        let bytes = self.read(rel)?;
        let expected = self
            .sums
            .get(rel)
            .ok_or_else(|| Error::Manifest(format!("{rel} is not in sha256sums.txt")))?;
        check(rel, &bytes, expected)?;
        let bytes: Arc<[u8]> = bytes.into();
        self.containers.insert(rel.to_owned(), bytes.clone());
        Ok(bytes)
    }
}

fn zip_member(container: &[u8], member: &str) -> Result<Option<Vec<u8>>, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(container))
        .map_err(|e| Error::Io(format!("reading zip: {e}")))?;
    let mut entry = match zip.by_name(member) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(Error::Io(format!("{member}: {e}"))),
    };
    let mut out = Vec::new();
    entry
        .read_to_end(&mut out)
        .map_err(|e| Error::Io(format!("{member}: {e}")))?;
    Ok(Some(out))
}

impl Store {
    /// Reads and verifies every file in `manifest` from the archive at
    /// `archive_root`.
    ///
    /// # Errors
    /// Fails on the first file that is missing, an LFS pointer, or does not
    /// match its recorded hash, and if the page shell cannot be rewritten.
    pub fn load(archive_root: &Path, manifest: &BootManifest) -> Result<Self, Error> {
        let mut archive = Archive::open(archive_root)?;
        let page_raw = archive.read(&manifest.page_shell.archive_path)?;
        check("page shell", &page_raw, &manifest.page_shell.sha256)?;
        let page = page::rewrite(&page_raw)?;

        let mut by_path = HashMap::new();
        let mut apk = None;
        for route in &manifest.routes {
            let bytes = if let Some(member) = &route.member {
                let container = archive.container(&route.archive_path)?;
                if member.starts_with(APK_WWW) {
                    apk = Some(container.clone());
                }
                zip_member(&container, member)?.ok_or_else(|| {
                    Error::Manifest(format!("{member} missing from {}", route.archive_path))
                })?
            } else {
                archive.read(&route.archive_path)?
            };
            check(&route.path, &bytes, &route.sha256)?;
            by_path.insert(
                route.path.clone(),
                Asset {
                    bytes: bytes.into(),
                    content_type: content_type(&route.path),
                },
            );
        }
        if !by_path.contains_key("/js/app.js") {
            return Err(Error::Manifest("manifest has no /js/app.js route".into()));
        }
        Ok(Self {
            page: Asset {
                bytes: page.into(),
                content_type: "text/html; charset=utf-8",
            },
            by_path,
            apk,
            apk_prefix: APK_WWW.to_owned(),
        })
    }

    /// The rewritten page shell.
    #[must_use]
    pub fn page(&self) -> &Asset {
        &self.page
    }

    /// Looks up a request path. Paths the manifest lists come first; other
    /// paths under `/images/` are read from the APK.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<Asset> {
        if let Some(a) = self.by_path.get(path) {
            return Some(a.clone());
        }
        let rel = path.strip_prefix('/')?;
        if !rel.starts_with("images/") || rel.split('/').any(|s| s == ".." || s.is_empty()) {
            return None;
        }
        let apk = self.apk.as_ref()?;
        let member = format!("{}{rel}", self.apk_prefix);
        let bytes = zip_member(apk, &member).ok()??;
        Some(Asset {
            bytes: bytes.into(),
            content_type: content_type(path),
        })
    }

    /// Number of manifest routes loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_types() {
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
        assert_eq!(content_type("/js/lib/zip.js"), "application/javascript");
        assert_eq!(content_type("/res.zip"), "application/zip");
    }

    #[test]
    fn committed_manifest_parses() {
        let m = BootManifest::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data/boot/20160806061006.json"
        )))
        .unwrap();
        assert_eq!(m.build, "20160806061006");
        assert!(
            m.routes
                .iter()
                .any(|r| r.path == "/js/app.js" && r.sha256 == m.client.sha256)
        );
        assert!(m.routes.iter().any(|r| r.path == "/res.zip"));
    }
}

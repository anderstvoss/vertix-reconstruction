//! Cosmetic art and mod packs restored from the archive.
//!
//! Recovered files override the client build's copies: every file named
//! here is answered before the request falls through to the client build
//! (`client_dir`), which keeps serving whatever the archive lacks (KRP's
//! later hats, the shirts, sprays 44 and up). Nothing original is part of
//! this repository; `data/content/` only says where each file lives in a
//! vertix-archive clone and what its SHA-256 is (`scripts/import_content.py`).
//!
//! - Cosmetics (`cosmetics.json`): hats, shirts, camos and sprays from
//!   first-party captures (the Aug-2016 Android APK, Wayback). Where a file
//!   has several first-party versions, `[content] date` picks the one that
//!   was live on that day.
//! - Mod packs (`mods.json`): community `vertixmod.zip` packs, served at
//!   `/mods/<key>/vertixmod.zip` (and at their old Dropbox key), listed at
//!   `/mods/`. A pack's first available source wins: an archive file, or
//!   the copy `scripts/extract_mods.py` unpacked into `mods_dir`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;
use serde_json::json;

use crate::config::Content as ContentConfig;
use crate::originals::{Archive, Error as ArchiveError, content_type, sha256_of};

#[derive(Debug, Deserialize)]
struct CosmeticsManifest {
    files: Vec<CosmeticFile>,
}

#[derive(Debug, Deserialize)]
struct CosmeticFile {
    key: String,
    family: String,
    paths: Vec<String>,
    versions: Vec<CosmeticVersion>,
}

#[derive(Debug, Deserialize)]
struct CosmeticVersion {
    sha256: String,
    first_seen: String,
    archive_path: String,
    member: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModsManifest {
    packs: Vec<PackEntry>,
}

#[derive(Debug, Deserialize)]
struct PackEntry {
    key: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    sources: Vec<PackSource>,
}

#[derive(Debug, Deserialize)]
struct PackSource {
    source: SourceRef,
    sha256: String,
    bytes: u64,
    sprites: u32,
    real_sounds: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum SourceRef {
    Archive {
        archive_path: String,
        #[serde(default)]
        capture: String,
    },
    Bundle {
        bundle: String,
    },
}

/// A mod pack ready to serve.
#[derive(Debug, Clone)]
pub struct Pack {
    pub key: String,
    pub name: String,
    pub aliases: Vec<String>,
    /// The verified file, read again for each request.
    pub path: PathBuf,
    pub bytes: u64,
    pub sprites: u32,
    pub real_sounds: u32,
    /// Where the copy came from, for the list.
    pub origin: String,
}

/// Everything restored, verified and ready to serve.
#[derive(Debug, Default)]
pub struct Content {
    files: HashMap<String, (Bytes, &'static str)>,
    packs: Vec<Pack>,
    /// Restored cosmetic files per family.
    pub counts: HashMap<String, usize>,
    /// Cosmetic files whose only version was first seen after `date`.
    pub later_only: usize,
    /// Packs listed but not available here, by key.
    pub missing_packs: Vec<String>,
}

impl Content {
    /// Loads what the config enables.
    ///
    /// # Errors
    /// Fails if a manifest cannot be read, or a file is present but does
    /// not match its recorded hash. A file that is absent is skipped, so
    /// the client build's copy is served instead.
    pub fn load(cfg: &ContentConfig, archive: Option<&Path>) -> Result<Self, String> {
        let mut content = Self::default();
        if cfg.cosmetics {
            if let Some(root) = archive {
                content.load_cosmetics(&cfg.cosmetics_manifest, &cfg.date, root)?;
            }
        }
        if cfg.mods {
            content.load_mods(&cfg.mods_manifest, &cfg.mods_dir, archive)?;
        }
        Ok(content)
    }

    fn load_cosmetics(&mut self, manifest: &Path, date: &str, root: &Path) -> Result<(), String> {
        let m: CosmeticsManifest = read_json(manifest)?;
        let mut archive = Archive::open(root).map_err(|e| format!("archive: {e}"))?;
        for f in &m.files {
            let Some((v, later)) = pick_version(&f.versions, date) else {
                continue;
            };
            let bytes = match archive.file(&v.archive_path, v.member.as_deref(), &v.sha256) {
                Ok(b) => b,
                Err(ArchiveError::HashMismatch { .. }) => {
                    return Err(format!("{}: archive copy does not match its hash", f.key));
                }
                // Not pulled or not present: the client build's copy stays.
                Err(_) => continue,
            };
            let bytes = Bytes::from(bytes);
            for p in &f.paths {
                self.files
                    .insert(p.clone(), (bytes.clone(), content_type(p)));
            }
            *self.counts.entry(f.family.clone()).or_default() += 1;
            self.later_only += usize::from(later);
        }
        Ok(())
    }

    fn load_mods(
        &mut self,
        manifest: &Path,
        mods_dir: &Path,
        archive: Option<&Path>,
    ) -> Result<(), String> {
        let m: ModsManifest = read_json(manifest)?;
        for entry in m.packs {
            let mut found = None;
            for src in &entry.sources {
                let (path, origin) = match &src.source {
                    SourceRef::Archive {
                        archive_path,
                        capture,
                    } => match archive {
                        Some(root) => (root.join(archive_path), format!("Wayback {capture}")),
                        None => continue,
                    },
                    SourceRef::Bundle { bundle } => (
                        mods_dir.join(&entry.key).join("vertixmod.zip"),
                        format!(
                            "fan repository {}",
                            bundle.rsplit('/').next().unwrap_or(bundle)
                        ),
                    ),
                };
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                if sha256_of(&bytes) != src.sha256 {
                    return Err(format!(
                        "mod pack {}: {} does not match its hash",
                        entry.key,
                        path.display()
                    ));
                }
                found = Some(Pack {
                    key: entry.key.clone(),
                    name: entry.name.clone(),
                    aliases: entry.aliases.clone(),
                    path,
                    bytes: src.bytes,
                    sprites: src.sprites,
                    real_sounds: src.real_sounds,
                    origin,
                });
                break;
            }
            match found {
                Some(p) => self.packs.push(p),
                None => self.missing_packs.push(entry.key),
            }
        }
        Ok(())
    }

    /// Number of URL paths answered from cosmetics.
    #[must_use]
    pub fn cosmetic_paths(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn packs(&self) -> &[Pack] {
        &self.packs
    }

    fn pack(&self, key: &str) -> Option<&Pack> {
        self.packs
            .iter()
            .find(|p| p.key == key || p.aliases.iter().any(|a| a == key))
    }

    /// One line for the start-up log.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut fams: Vec<_> = self.counts.iter().collect();
        fams.sort();
        let fams: Vec<String> = fams.iter().map(|(k, v)| format!("{k} {v}")).collect();
        let mut s = format!(
            "content: {} cosmetic files from the archive ({}), {} mod packs",
            self.counts.values().sum::<usize>(),
            if fams.is_empty() {
                "none".to_owned()
            } else {
                fams.join(", ")
            },
            self.packs.len()
        );
        if !self.missing_packs.is_empty() {
            let _ = write!(
                s,
                "; {} packs not unpacked (scripts/extract_mods.py)",
                self.missing_packs.len()
            );
        }
        s
    }
}

/// The version live on `date` (the latest first seen on or before it),
/// else the earliest; `true` when only later versions exist.
fn pick_version<'a>(
    versions: &'a [CosmeticVersion],
    date: &str,
) -> Option<(&'a CosmeticVersion, bool)> {
    versions
        .iter()
        .filter(|v| v.first_seen.as_str() <= date)
        .max_by(|a, b| a.first_seen.cmp(&b.first_seen))
        .map(|v| (v, false))
        .or_else(|| {
            versions
                .iter()
                .min_by(|a, b| a.first_seen.cmp(&b.first_seen))
                .map(|v| (v, true))
        })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Routes for every restored file, to merge ahead of the client build.
pub fn router(content: Arc<Content>) -> Router {
    let mut r = Router::new()
        .route("/mods/", get(mod_list))
        .route("/mods/index.json", get(mod_index))
        .route("/mods/{key}/vertixmod.zip", get(mod_pack));
    for (path, (bytes, ctype)) in &content.files {
        let (bytes, ctype) = (bytes.clone(), *ctype);
        r = r.route(
            path,
            get(move || async move {
                ([(header::CONTENT_TYPE, ctype)], Body::from(bytes.clone())).into_response()
            }),
        );
    }
    r.with_state(content)
}

async fn mod_pack(State(c): State<Arc<Content>>, UrlPath(key): UrlPath<String>) -> Response {
    let Some(pack) = c.pack(&key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(&pack.path).await {
        Ok(bytes) => {
            let mut res = Response::new(Body::from(bytes));
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/zip"),
            );
            res
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn mod_index(State(c): State<Arc<Content>>) -> Response {
    let packs: Vec<_> = c
        .packs
        .iter()
        .map(|p| {
            json!({
                "key": p.key,
                "name": p.name,
                "aliases": p.aliases,
                "url": format!("/mods/{}/vertixmod.zip", p.key),
                "bytes": p.bytes,
                "sprites": p.sprites,
                "sounds": p.real_sounds,
                "origin": p.origin,
            })
        })
        .collect();
    (
        [(header::CONTENT_TYPE, "application/json")],
        json!({ "packs": packs, "missing": c.missing_packs }).to_string(),
    )
        .into_response()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn mod_list(State(c): State<Arc<Content>>) -> Response {
    let mut rows = String::new();
    for p in &c.packs {
        let _ = write!(
            rows,
            "<tr><td>{}</td><td><code>{}</code></td><td>{} KB</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape(&p.name),
            escape(&p.key),
            p.bytes.div_ceil(1024),
            p.sprites,
            p.real_sounds,
            escape(&p.origin),
        );
    }
    let page = format!(
        "<!doctype html><meta charset=utf-8><title>Mod packs</title>\
         <style>body{{font:14px sans-serif;margin:2em;background:#222;color:#eee}}\
         td,th{{padding:4px 10px;text-align:left}}code{{color:#9cf}}</style>\
         <h1>Mod packs</h1><p>Type a key into the game's MODS tab and press LOAD. \
         Packs are community works, restored from the archive.</p>\
         <table><tr><th>Pack</th><th>Key</th><th>Size</th><th>Sprites</th>\
         <th>Sounds</th><th>Copy</th></tr>{rows}</table>"
    );
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], page).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(first_seen: &str) -> CosmeticVersion {
        CosmeticVersion {
            sha256: first_seen.to_owned(),
            first_seen: first_seen.to_owned(),
            archive_path: String::new(),
            member: None,
        }
    }

    #[test]
    fn picks_the_version_live_on_the_day() {
        let vs = [v("2016-08-04"), v("2020-10-17")];
        assert_eq!(
            pick_version(&vs, "2017-07-01").unwrap().0.first_seen,
            "2016-08-04"
        );
        assert_eq!(
            pick_version(&vs, "2021-01-01").unwrap().0.first_seen,
            "2020-10-17"
        );
        let later = pick_version(&vs, "2016-01-01").unwrap();
        assert_eq!((later.0.first_seen.as_str(), later.1), ("2016-08-04", true));
        assert!(pick_version(&[], "2017-07-01").is_none());
    }

    #[test]
    fn committed_manifests_parse() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/data/content"));
        let c: CosmeticsManifest = read_json(&dir.join("cosmetics.json")).unwrap();
        assert!(
            c.files
                .iter()
                .all(|f| !f.versions.is_empty() && !f.paths.is_empty())
        );
        let m: ModsManifest = read_json(&dir.join("mods.json")).unwrap();
        assert!(m.packs.iter().all(|p| !p.sources.is_empty()));
    }

    #[test]
    fn packs_are_found_by_key_or_alias() {
        let c = Content {
            packs: vec![Pack {
                key: "sonic-mod-primary".into(),
                name: "Sonic".into(),
                aliases: vec!["13xlc5n3ipudqsn".into()],
                path: PathBuf::new(),
                bytes: 0,
                sprites: 0,
                real_sounds: 0,
                origin: String::new(),
            }],
            ..Content::default()
        };
        assert!(c.pack("sonic-mod-primary").is_some());
        assert!(c.pack("13xlc5n3ipudqsn").is_some());
        assert!(c.pack("pacman").is_none());
    }
}

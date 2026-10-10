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
        /// A zip member of `archive_path` (the APK's res.zip), if set.
        #[serde(default)]
        member: Option<String>,
        /// Where the copy came from, for the list.
        origin: String,
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
    pub data: PackData,
    pub bytes: u64,
    pub sprites: u32,
    pub real_sounds: u32,
    /// Where the copy came from, for the list.
    pub origin: String,
}

/// Where a pack's verified bytes are.
#[derive(Debug, Clone)]
pub enum PackData {
    /// A large file, read again for each request.
    File(PathBuf),
    /// A pack read out of a container (the APK), kept in memory.
    Memory(Bytes),
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
    /// Sprays from `sprays_dir` (id, name, added rather than replaced).
    pub folder_sprays: Vec<(u64, String, bool)>,
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
                let Some((data, origin)) =
                    pack_source(&src.source, &src.sha256, &entry.key, mods_dir, archive)?
                else {
                    continue;
                };
                found = Some(Pack {
                    key: entry.key.clone(),
                    name: entry.name.clone(),
                    aliases: entry.aliases.clone(),
                    data,
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

/// Reads and verifies one source of a pack; `None` if it is not here.
fn pack_source(
    source: &SourceRef,
    sha256: &str,
    key: &str,
    mods_dir: &Path,
    archive: Option<&Path>,
) -> Result<Option<(PackData, String)>, String> {
    let mismatch = |what: &str| format!("mod pack {key}: {what} does not match its hash");
    match source {
        SourceRef::Archive {
            archive_path,
            member: Some(member),
            origin,
        } => {
            let Some(root) = archive else { return Ok(None) };
            let mut a = Archive::open(root).map_err(|e| format!("archive: {e}"))?;
            match a.file(archive_path, Some(member), sha256) {
                Ok(b) => Ok(Some((PackData::Memory(Bytes::from(b)), origin.clone()))),
                Err(ArchiveError::HashMismatch { .. }) => Err(mismatch(member)),
                Err(_) => Ok(None),
            }
        }
        SourceRef::Archive {
            archive_path,
            member: None,
            origin,
        } => {
            let Some(root) = archive else { return Ok(None) };
            let path = root.join(archive_path);
            verify_file(&path, sha256, &mismatch)
                .map(|ok| ok.map(|()| (PackData::File(path), origin.clone())))
        }
        SourceRef::Bundle { bundle } => {
            let path = mods_dir.join(key).join("vertixmod.zip");
            let origin = format!(
                "fan repository {}",
                bundle.rsplit('/').next().unwrap_or(bundle)
            );
            verify_file(&path, sha256, &mismatch)
                .map(|ok| ok.map(|()| (PackData::File(path), origin)))
        }
    }
}

fn verify_file(
    path: &Path,
    sha256: &str,
    mismatch: &dyn Fn(&str) -> String,
) -> Result<Option<()>, String> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(None);
    };
    if sha256_of(&bytes) == sha256 {
        Ok(Some(()))
    } else {
        Err(mismatch(&path.display().to_string()))
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

/// Largest spray image read from `sprays_dir`.
const MAX_SPRAY_BYTES: u64 = 4 << 20;

impl Content {
    /// Reads every PNG in `dir` as a spray (see `[content] sprays_dir`).
    /// `<id>.png` replaces spray `id`'s image; any other file is a new
    /// spray, numbered after the highest id in `sprays` in file-name order
    /// and named after the file. New sprays are appended to `sprays` with
    /// KRP's default display values, which the patched client ignores (it
    /// sizes a spray from its image). Returns warnings for skipped files.
    pub fn add_folder_sprays(
        &mut self,
        dir: &Path,
        sprays: &mut Vec<serde_json::Value>,
    ) -> Vec<String> {
        let mut warnings = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return warnings;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
            .collect();
        files.sort();
        let id_of = |s: &serde_json::Value| s.get("id").and_then(serde_json::Value::as_u64);
        let mut next = sprays.iter().filter_map(id_of).max().unwrap_or(0);
        for path in files {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bytes = match std::fs::metadata(&path) {
                Ok(m) if m.len() > MAX_SPRAY_BYTES => {
                    warnings.push(format!("{}: larger than 4 MB, skipped", path.display()));
                    continue;
                }
                Ok(_) => match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => {
                        warnings.push(format!("{}: {e}", path.display()));
                        continue;
                    }
                },
                Err(e) => {
                    warnings.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                warnings.push(format!("{}: not a PNG image, skipped", path.display()));
                continue;
            }
            let (id, added) = match stem.parse::<u64>() {
                Ok(id) if id > 0 => (id, !sprays.iter().any(|s| id_of(s) == Some(id))),
                _ => {
                    next += 1;
                    (next, true)
                }
            };
            let name = if stem.parse::<u64>().is_ok() && !added {
                sprays
                    .iter()
                    .find(|s| id_of(s) == Some(id))
                    .and_then(|s| s.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            } else {
                stem.replace(['_', '-'], " ")
            };
            if added {
                next = next.max(id);
                sprays.push(json!({
                    "id": id,
                    "name": name,
                    "info": {"scale": 64, "alpha": 1, "resolution": 30},
                }));
            }
            let bytes = Bytes::from(bytes);
            for p in [
                format!("/assets/sprays/{id}.png"),
                format!("/images/sprays/{id}.png"),
            ] {
                self.files.insert(p, (bytes.clone(), "image/png"));
            }
            self.folder_sprays.push((id, name, added));
        }
        warnings
    }
}

/// Routes for every restored file, to merge ahead of the client build.
pub fn router(content: Arc<Content>) -> Router {
    let mut r = Router::new()
        .route("/mods/", get(mod_list))
        .route("/mods/index.json", get(mod_index))
        .route("/mods/{key}/vertixmod.zip", get(mod_pack))
        .route("/sprays/index.json", get(spray_index));
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
    let bytes = match &pack.data {
        PackData::Memory(b) => Ok(b.clone()),
        PackData::File(path) => tokio::fs::read(path).await.map(Bytes::from),
    };
    match bytes {
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

/// The sprays added from `sprays_dir`, for the client's spray list.
async fn spray_index(State(c): State<Arc<Content>>) -> Response {
    let sprays: Vec<_> = c
        .folder_sprays
        .iter()
        .filter(|(_, _, added)| *added)
        .map(|(id, name, _)| json!({"id": id, "name": name, "info": {"scale": 64, "alpha": 1, "resolution": 30}}))
        .collect();
    (
        [(header::CONTENT_TYPE, "application/json")],
        json!({ "sprays": sprays }).to_string(),
    )
        .into_response()
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
    fn png_files_in_the_sprays_folder_become_sprays() {
        let dir = std::env::temp_dir().join(format!("vertix-sprays-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        std::fs::write(dir.join("2.png"), &png).unwrap();
        std::fs::write(dir.join("My_Spray.png"), &png).unwrap();
        std::fs::write(dir.join("b-side.PNG"), &png).unwrap();
        std::fs::write(dir.join("broken.png"), b"not a png").unwrap();
        std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();
        let mut sprays = vec![
            json!({"id": 1, "name": "Strike"}),
            json!({"id": 2, "name": "Schweiz"}),
        ];
        let mut c = Content::default();
        let warnings = c.add_folder_sprays(&dir, &mut sprays);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(
            c.folder_sprays,
            vec![
                (2, "Schweiz".to_owned(), false),
                (3, "My Spray".to_owned(), true),
                (4, "b side".to_owned(), true),
            ]
        );
        let ids: Vec<_> = sprays.iter().map(|s| s["id"].clone()).collect();
        assert_eq!(ids, [json!(1), json!(2), json!(3), json!(4)]);
        for p in [
            "/assets/sprays/2.png",
            "/images/sprays/3.png",
            "/assets/sprays/4.png",
        ] {
            assert_eq!(c.files[p].0.as_ref(), png.as_slice(), "{p}");
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
                data: PackData::Memory(Bytes::new()),
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

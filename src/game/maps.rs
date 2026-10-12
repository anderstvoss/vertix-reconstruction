//! Where maps come from, behind one trait so a source can be swapped when
//! better map evidence turns up.
//!
//! The map list is data, not code: a source offers whatever maps it finds,
//! several sources can be combined, and each mode's `maps` list (in the rule
//! layers) names the ids it plays. Adding or replacing a map as new
//! evidence turns up means adding a file and, if a mode should play it, an
//! id in a rule layer.
//!
//! - [`ArchiveGenData`]: every `map-<id>.genData.json` in a directory of a
//!   vertix-archive clone, each hash-checked against the archive's
//!   `sha256sums.txt`. By default that is KRP's 24 maps, whose numbering
//!   the mode lists use (PROVISIONAL: no original map file survives).
//! - [`TextFiles`]: maps in this repository's text format, keyed by file
//!   stem, for our own layouts.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::data::Mode;
use super::map::{Error, Map};
use crate::originals::Archive;

/// One loadable map.
#[derive(Debug, Clone)]
pub struct MapEntry {
    /// The id mode tables refer to.
    pub id: String,
    /// Where it came from, for logs.
    pub source: String,
    pub map: Map,
}

/// A source of maps.
pub trait MapSource {
    /// Loads every map the source offers.
    ///
    /// # Errors
    /// Fails if a map cannot be read, verified or parsed.
    fn load(&self) -> Result<Vec<MapEntry>, Error>;
}

/// Default directory of the map candidates inside the archive.
pub const ARCHIVE_MAP_DIR: &str = "vertix-preservation/derived/maps/krp-2026-candidates";

/// The `map-<id>.genData.json` files in one archive directory.
pub struct ArchiveGenData {
    pub root: PathBuf,
    /// Relative to `root`, with `/` separators as in `sha256sums.txt`.
    pub dir: String,
}

impl ArchiveGenData {
    /// The map ids present in the directory, numbers first in numeric order.
    fn ids(&self) -> Result<Vec<String>, Error> {
        let path = self.root.join(&self.dir);
        let read =
            std::fs::read_dir(&path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let mut ids: Vec<String> = read
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let id = name.strip_prefix("map-")?.strip_suffix(".genData.json")?;
                (!id.is_empty()).then(|| id.to_owned())
            })
            .collect();
        ids.sort_by(|a, b| match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        });
        Ok(ids)
    }
}

impl MapSource for ArchiveGenData {
    fn load(&self) -> Result<Vec<MapEntry>, Error> {
        let archive = Archive::open(&self.root).map_err(|e| Error(e.to_string()))?;
        let dir = self.dir.trim_end_matches('/');
        self.ids()?
            .into_iter()
            .map(|id| {
                let rel = format!("{dir}/map-{id}.genData.json");
                let bytes = archive.verified(&rel).map_err(|e| Error(e.to_string()))?;
                let doc: Value =
                    serde_json::from_slice(&bytes).map_err(|e| Error(format!("{rel}: {e}")))?;
                let gen_data = doc
                    .get("genData")
                    .ok_or_else(|| Error(format!("{rel}: no genData")))?;
                let map =
                    Map::from_gen_data(gen_data).map_err(|e| Error(format!("{rel}: {}", e.0)))?;
                Ok(MapEntry {
                    id,
                    source: rel,
                    map,
                })
            })
            .collect()
    }
}

/// Text maps, each keyed by its file stem.
pub struct TextFiles {
    pub files: Vec<PathBuf>,
}

impl MapSource for TextFiles {
    fn load(&self) -> Result<Vec<MapEntry>, Error> {
        self.files
            .iter()
            .map(|p| {
                let text = std::fs::read_to_string(p)
                    .map_err(|e| Error(format!("{}: {e}", p.display())))?;
                let map =
                    Map::parse(&text).map_err(|e| Error(format!("{}: {}", p.display(), e.0)))?;
                Ok(MapEntry {
                    id: stem(p),
                    source: p.display().to_string(),
                    map,
                })
            })
            .collect()
    }
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

/// The loaded maps.
#[derive(Debug, Clone)]
pub struct MapSet {
    entries: Vec<MapEntry>,
}

impl MapSet {
    /// Wraps loaded maps. When several sources offer the same id, the
    /// later one replaces the earlier, keeping its place in the list.
    ///
    /// # Errors
    /// Fails if there are none.
    pub fn new(loaded: Vec<MapEntry>) -> Result<Self, Error> {
        let mut entries: Vec<MapEntry> = Vec::with_capacity(loaded.len());
        for e in loaded {
            match entries.iter_mut().find(|x| x.id == e.id) {
                Some(slot) => *slot = e,
                None => entries.push(e),
            }
        }
        if entries.is_empty() {
            return Err(Error("no maps loaded".into()));
        }
        Ok(Self { entries })
    }

    /// The loaded ids, in order.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.id.as_str()).collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The maps a mode may play: those its `maps` list names, in that
    /// order, or every map if none of them is loaded (for example with
    /// our own text maps).
    #[must_use]
    pub fn for_mode(&self, mode: &Mode) -> Vec<&MapEntry> {
        let listed: Vec<&MapEntry> = mode
            .maps
            .iter()
            .filter_map(|id| self.entries.iter().find(|e| &e.id == id))
            .collect();
        if listed.is_empty() {
            self.entries.iter().collect()
        } else {
            listed
        }
    }

    /// The map with this id, if loaded.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&MapEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Picks a map for a mode with a random number.
    #[must_use]
    pub fn pick(&self, mode: &Mode, random: u64) -> (String, Map) {
        let options = self.for_mode(mode);
        let n = u64::try_from(options.len()).unwrap_or(1);
        let e = options[usize::try_from(random % n).unwrap_or(0)];
        (e.id.clone(), e.map.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::data::committed;

    fn entry(id: &str) -> MapEntry {
        MapEntry {
            id: id.into(),
            source: "test".into(),
            map: Map::parse(include_str!("../../data/maps/arena.txt")).unwrap(),
        }
    }

    #[test]
    fn modes_pick_from_their_own_list() {
        let rules = committed("krp");
        let tdm = rules.modes.iter().find(|m| m.code == "tdm").unwrap();
        let set = MapSet::new(["13", "14", "15"].map(entry).to_vec()).unwrap();
        let ids: Vec<&str> = set.for_mode(tdm).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["13", "15"]);
        for r in 0..8 {
            assert_ne!(set.pick(tdm, r).0, "14");
        }
        // None of a mode's maps loaded: play whatever there is.
        let own = MapSet::new(vec![entry("arena")]).unwrap();
        assert_eq!(own.pick(tdm, 3).0, "arena");
        assert!(MapSet::new(Vec::new()).is_err());
    }

    #[test]
    fn later_sources_replace_maps_with_the_same_id() {
        let mut newer = entry("3");
        newer.source = "newer".into();
        let set = MapSet::new(vec![entry("3"), entry("5"), newer]).unwrap();
        assert_eq!(set.ids(), ["3", "5"]);
        assert_eq!(set.entries[0].source, "newer");
    }

    #[test]
    fn archive_source_finds_whatever_maps_the_directory_holds() {
        let root = std::env::temp_dir().join(format!("vertix-maps-{}", std::process::id()));
        let dir = root.join("maps");
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "map-10.genData.json",
            "map-2.genData.json",
            "map-new.genData.json",
            "notes.md",
        ] {
            std::fs::write(dir.join(name), "{}").unwrap();
        }
        let src = ArchiveGenData {
            root: root.clone(),
            dir: "maps".into(),
        };
        assert_eq!(src.ids().unwrap(), ["2", "10", "new"]);
        // No sha256sums.txt: nothing unverified is loaded.
        assert!(src.load().is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

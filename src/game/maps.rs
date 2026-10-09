//! Where maps come from, behind one trait so a source can be swapped when
//! better map evidence turns up.
//!
//! - [`ArchiveGenData`]: the 24 `KrunkerRevival` map candidates, read at run
//!   time from a vertix-archive clone and hash-checked. Their numbering is
//!   the one the mode table's `maps` lists use. They are PROVISIONAL
//!   (Anders, 2026-10-09) and are never committed here: the source has no
//!   license.
//! - [`TextFiles`]: maps in this repository's text format, keyed by file
//!   stem, for our own layouts.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::assumptions::Mode;
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
    fn load(&self, tile_scale: f64) -> Result<Vec<MapEntry>, Error>;
}

/// Directory of the `KrunkerRevival` candidates inside the archive.
pub const ARCHIVE_MAP_DIR: &str = "vertix-preservation/derived/maps/krp-2026-candidates";
/// How many candidates the archive holds (`map-0` to `map-23`).
pub const ARCHIVE_MAP_COUNT: usize = 24;

/// The archive's `map-N.genData.json` files.
pub struct ArchiveGenData {
    pub root: PathBuf,
}

impl MapSource for ArchiveGenData {
    fn load(&self, tile_scale: f64) -> Result<Vec<MapEntry>, Error> {
        let archive = Archive::open(&self.root).map_err(|e| Error(e.to_string()))?;
        (0..ARCHIVE_MAP_COUNT)
            .map(|n| {
                let rel = format!("{ARCHIVE_MAP_DIR}/map-{n}.genData.json");
                let bytes = archive.verified(&rel).map_err(|e| Error(e.to_string()))?;
                let doc: Value =
                    serde_json::from_slice(&bytes).map_err(|e| Error(format!("{rel}: {e}")))?;
                let gen_data = doc
                    .get("genData")
                    .ok_or_else(|| Error(format!("{rel}: no genData")))?;
                let map = Map::from_gen_data(gen_data, tile_scale, false)
                    .map_err(|e| Error(format!("{rel}: {}", e.0)))?;
                Ok(MapEntry {
                    id: n.to_string(),
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
    fn load(&self, tile_scale: f64) -> Result<Vec<MapEntry>, Error> {
        self.files
            .iter()
            .map(|p| {
                let text = std::fs::read_to_string(p)
                    .map_err(|e| Error(format!("{}: {e}", p.display())))?;
                let map = Map::parse(&text, tile_scale, false)
                    .map_err(|e| Error(format!("{}: {}", p.display(), e.0)))?;
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
    /// Wraps loaded maps.
    ///
    /// # Errors
    /// Fails if there are none.
    pub fn new(entries: Vec<MapEntry>) -> Result<Self, Error> {
        if entries.is_empty() {
            return Err(Error("no maps loaded".into()));
        }
        Ok(Self { entries })
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

    /// Picks a map for a mode with a random number, laid out for the mode.
    #[must_use]
    pub fn pick(&self, mode: &Mode, random: u64) -> (String, Map) {
        let options = self.for_mode(mode);
        let n = u64::try_from(options.len()).unwrap_or(1);
        let e = options[usize::try_from(random % n).unwrap_or(0)];
        (e.id.clone(), e.map.for_mode(&mode.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::assumptions::committed;

    fn entry(id: &str) -> MapEntry {
        MapEntry {
            id: id.into(),
            source: "test".into(),
            map: Map::parse(include_str!("../../data/maps/arena.txt"), 100.0, false).unwrap(),
        }
    }

    #[test]
    fn modes_pick_from_their_own_list() {
        let rules = committed().0;
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
    fn missing_archive_is_an_error() {
        let src = ArchiveGenData {
            root: PathBuf::from("does-not-exist"),
        };
        assert!(src.load(100.0).is_err());
    }
}

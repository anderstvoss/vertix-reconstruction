//! Typed view of the rule layers in `data/rules/`.
//!
//! Game logic reads numbers only from here, never from literals. The
//! numbers come from a stack of TOML layers: a provisional base, then
//! dated evidence for the target build, then optional local tuning. A
//! later layer replaces individual values of an earlier one, and every
//! value keeps the status and source of the layer entry that set it, so a
//! correction is a data change and [`Provenance`] can say where each
//! number came from.
//!
//! Layer format:
//!
//! ```toml
//! [layer]
//! status = "PROVISIONAL"          # default for every value in this file
//! source = "where these come from"
//!
//! [world]
//! status = "RECOVERED"            # optional: overrides [layer] for this table
//! source = "app.js defaults"
//! view_mult = 1
//!
//! [[weapons]]                     # arrays of tables merge by `name`
//! name = "smg"
//! status = "INFERRED"             # optional: for the fields of this entry
//! source = "wiki, 2016-07-22"
//! reloadSpeed = 800
//! ```
//!
//! The same `name` may appear more than once in one layer, so values with
//! different sources can sit in separate entries.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use toml::{Table, Value};

/// Evidence words, from strongest to weakest. They follow the research
/// repository's evidence ledger.
pub const STATUSES: &[&str] = &[
    // Read from first-party client code or patch notes.
    "RECOVERED",
    // From a contemporaneous community source (the 2016 wiki).
    "INFERRED",
    // Decided by the project owner.
    "DECIDED",
    // A starting value from a non-authoritative later source (KRP).
    "PROVISIONAL",
    // Our own choice, with no source at all.
    "ASSUMED",
];

#[derive(Debug, Clone, Deserialize)]
pub struct Assumptions {
    pub world: World,
    pub net: Net,
    pub round: Round,
    pub modes: Vec<Mode>,
    pub player: PlayerRules,
    pub classes: Vec<Class>,
    pub weapons: Vec<WeaponSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct World {
    pub tile_scale: f64,
    pub max_screen_width: f64,
    pub max_screen_height: f64,
    pub view_mult: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Net {
    /// How often the server advances the world and sends `rsd`.
    pub update_hz: f64,
    pub max_input_delta_ms: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Round {
    /// `code` of the mode new rooms play.
    pub mode: String,
}

/// A game mode. The client reads the fields it is sent in
/// `mapData.gameMode`; the rest stay on the server.
#[derive(Debug, Clone, Deserialize)]
pub struct Mode {
    pub name: String,
    pub code: String,
    pub desc1: String,
    pub desc2: String,
    pub score: u32,
    pub teams: bool,
    #[serde(default = "one")]
    pub kill_score_mult: f64,
    /// Map ids this mode rotates through.
    #[serde(default)]
    pub maps: Vec<String>,
    /// Class every player plays, whatever they picked (by class name).
    #[serde(default)]
    pub forced_class: Option<String>,
}

fn one() -> f64 {
    1.0
}

impl Mode {
    /// `mapData.gameMode` as the client reads it.
    #[must_use]
    pub fn client_json(&self) -> Json {
        json!({
            "name": self.name,
            "code": self.code,
            "desc1": self.desc1,
            "desc2": self.desc2,
            "score": self.score,
            "teams": self.teams,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerRules {
    pub spawn_protection_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Class {
    pub name: String,
    pub weapons: Vec<usize>,
    pub max_health: f64,
    pub width: f64,
    pub height: f64,
    pub speed: f64,
    pub jump_strength: f64,
    pub gravity_strength: f64,
}

/// A weapon's static numbers, named as the client reads them.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponSpec {
    pub name: String,
    pub weapon_index: usize,
    pub dmg: f64,
    pub ammo: u32,
    pub max_ammo: u32,
    pub reload_speed: f64,
    pub fire_rate: f64,
    pub spread: Vec<f64>,
    pub width: f64,
    pub length: f64,
    pub y_offset: f64,
    pub hold_dist: f64,
    pub b_speed: f64,
    pub b_width: f64,
    pub b_height: f64,
    #[serde(default)]
    pub b_rand_scale: Option<[f64; 2]>,
    pub c_acc: f64,
    /// `maxLife` in ms; the client treats a missing or zero value as none.
    #[serde(default)]
    pub max_life: Option<f64>,
    pub bullets_per_shot: u32,
    pub pierce: u32,
    pub bounce: bool,
    pub dist_based: bool,
    pub explode_on_death: bool,
    pub b_dist: f64,
    pub b_trail: f64,
    pub b_sprite: u32,
    #[serde(default)]
    pub glow_width: Option<f64>,
    #[serde(default)]
    pub glow_height: Option<f64>,
    pub shake: f64,
}

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Where one value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub status: String,
    pub source: String,
    pub layer: String,
}

/// The origin of every value, keyed by path (`weapons.smg.reloadSpeed`).
#[derive(Debug, Clone, Default)]
pub struct Provenance {
    pub values: BTreeMap<String, (String, Origin)>,
    /// SHA-256 of the merged rules, to tie a trace to the exact numbers.
    pub hash: String,
}

impl Provenance {
    /// Count of values per status, in [`STATUSES`] order.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for s in STATUSES {
            let n = self.values.values().filter(|(_, o)| o.status == *s).count();
            if n > 0 {
                let _ = write!(out, "{}{n} {s}", if out.is_empty() { "" } else { ", " });
            }
        }
        out
    }

    /// One line per value: path, value, status, source.
    #[must_use]
    pub fn table(&self) -> String {
        let mut out = String::new();
        for (path, (value, o)) in &self.values {
            let _ = writeln!(
                out,
                "{path} = {value}\t{}\t{}\t[{}]",
                o.status, o.source, o.layer
            );
        }
        out
    }
}

#[derive(Clone)]
struct Meta {
    status: String,
    source: String,
}

impl Meta {
    fn within(&self, t: &Table, at: &str) -> Result<Self, Error> {
        let status = match t.get("status") {
            Some(Value::String(s)) => s.clone(),
            Some(_) => return Err(Error(format!("{at}: status must be a string"))),
            None => self.status.clone(),
        };
        if !STATUSES.contains(&status.as_str()) {
            return Err(Error(format!(
                "{at}: unknown status {status:?} (use one of {STATUSES:?})"
            )));
        }
        let source = match t.get("source") {
            Some(Value::String(s)) => s.clone(),
            Some(_) => return Err(Error(format!("{at}: source must be a string"))),
            None => self.source.clone(),
        };
        Ok(Self { status, source })
    }
}

fn is_meta(key: &str) -> bool {
    key == "status" || key == "source"
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

struct Merger<'a> {
    layer: &'a str,
    prov: &'a mut BTreeMap<String, (String, Origin)>,
}

impl Merger<'_> {
    fn record(&mut self, path: String, value: &Value, meta: &Meta) {
        self.prov.insert(
            path,
            (
                value.to_string(),
                Origin {
                    status: meta.status.clone(),
                    source: meta.source.clone(),
                    layer: self.layer.to_owned(),
                },
            ),
        );
    }

    fn table(
        &mut self,
        into: &mut Table,
        from: &Table,
        path: &str,
        meta: &Meta,
    ) -> Result<(), Error> {
        for (key, value) in from.iter().filter(|(k, _)| !is_meta(k)) {
            let here = join(path, key);
            match value {
                Value::Table(t) => {
                    let meta = meta.within(t, &here)?;
                    let slot = into
                        .entry(key.clone())
                        .or_insert_with(|| Value::Table(Table::new()));
                    let Value::Table(slot) = slot else {
                        return Err(Error(format!("{here}: a table here replaces a value")));
                    };
                    self.table(slot, t, &here, &meta)?;
                }
                Value::Array(items) if items.first().is_some_and(Value::is_table) => {
                    self.named_entries(into, key, items, &here, meta)?;
                }
                other => {
                    self.record(here, other, meta);
                    into.insert(key.clone(), other.clone());
                }
            }
        }
        Ok(())
    }

    /// Merges `[[key]]` entries into the existing list by their `name`.
    fn named_entries(
        &mut self,
        into: &mut Table,
        key: &str,
        items: &[Value],
        path: &str,
        meta: &Meta,
    ) -> Result<(), Error> {
        let slot = into
            .entry(key.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(list) = slot else {
            return Err(Error(format!("{path}: a list here replaces a value")));
        };
        for item in items {
            let Value::Table(entry) = item else {
                return Err(Error(format!("{path}: every entry must be a table")));
            };
            let Some(Value::String(name)) = entry.get("name") else {
                return Err(Error(format!("{path}: every entry needs a name")));
            };
            let here = join(path, name);
            let meta = meta.within(entry, &here)?;
            let at = list
                .iter()
                .position(|v| v.get("name").and_then(Value::as_str) == Some(name.as_str()));
            let i = at.unwrap_or_else(|| {
                list.push(Value::Table(Table::new()));
                list.len() - 1
            });
            let Some(Value::Table(target)) = list.get_mut(i) else {
                return Err(Error(format!("{here}: entry is not a table")));
            };
            let fields: Table = entry
                .iter()
                .filter(|(k, _)| !is_meta(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            self.table(target, &fields, &here, &meta)?;
        }
        Ok(())
    }
}

/// Merges layers in order and records where each value came from.
///
/// # Errors
/// Fails on malformed TOML, a missing `[layer]` header, an unknown status,
/// or a layer that changes a value's shape.
pub fn merge_layers(layers: &[(String, String)]) -> Result<(Table, Provenance), Error> {
    let mut merged = Table::new();
    let mut values = BTreeMap::new();
    for (label, text) in layers {
        let doc: Table = toml::from_str(text).map_err(|e| Error(format!("{label}: {e}")))?;
        let Some(Value::Table(head)) = doc.get("layer") else {
            return Err(Error(format!("{label}: missing [layer] status and source")));
        };
        let base = Meta {
            status: String::new(),
            source: String::new(),
        };
        let meta = base.within(head, &format!("{label} [layer]"))?;
        let body: Table = doc
            .iter()
            .filter(|(k, _)| k.as_str() != "layer")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Merger {
            layer: label,
            prov: &mut values,
        }
        .table(&mut merged, &body, "", &meta)?;
    }
    let canonical = serde_json::to_string(&merged).map_err(|e| Error(e.to_string()))?;
    let hash = Sha256::digest(canonical.as_bytes())
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
    Ok((merged, Provenance { values, hash }))
}

impl Assumptions {
    /// Loads and merges rule layer files, in order.
    ///
    /// # Errors
    /// Fails if a file is unreadable or the merged rules are inconsistent.
    pub fn load_layers(paths: &[PathBuf]) -> Result<(Self, Provenance), Error> {
        let mut layers = Vec::new();
        for p in paths {
            let text =
                std::fs::read_to_string(p).map_err(|e| Error(format!("{}: {e}", p.display())))?;
            layers.push((p.display().to_string(), text));
        }
        Self::from_layers(&layers)
    }

    /// Merges `(label, toml)` layers and checks the result.
    ///
    /// # Errors
    /// Fails if a layer is malformed or the merged rules are inconsistent.
    pub fn from_layers(layers: &[(String, String)]) -> Result<(Self, Provenance), Error> {
        let (merged, prov) = merge_layers(layers)?;
        let a: Self = Value::Table(merged)
            .try_into()
            .map_err(|e: toml::de::Error| Error(e.to_string()))?;
        a.validate()?;
        Ok((a, prov))
    }

    fn validate(&self) -> Result<(), Error> {
        if self.classes.is_empty() {
            return Err(Error("no classes".into()));
        }
        for (i, w) in self.weapons.iter().enumerate() {
            if w.weapon_index != i {
                return Err(Error(format!(
                    "weapons must be listed in weaponIndex order; entry {i} is {}",
                    w.weapon_index
                )));
            }
            if w.spread.is_empty() {
                return Err(Error(format!("weapon {} has an empty spread list", w.name)));
            }
        }
        for c in &self.classes {
            if let Some(&bad) = c.weapons.iter().find(|&&w| w >= self.weapons.len()) {
                return Err(Error(format!("class {} uses unknown weapon {bad}", c.name)));
            }
        }
        for m in &self.modes {
            if let Some(f) = &m.forced_class
                && !self.classes.iter().any(|c| &c.name == f)
            {
                return Err(Error(format!("mode {} forces unknown class {f}", m.code)));
            }
        }
        if self.mode().is_none() {
            return Err(Error(format!(
                "round.mode {} is not a mode",
                self.round.mode
            )));
        }
        if !(self.net.update_hz > 0.0 && self.net.update_hz <= 1000.0) {
            return Err(Error("net.update_hz must be in (0, 1000]".into()));
        }
        Ok(())
    }

    /// The mode new rooms play.
    #[must_use]
    pub fn mode(&self) -> Option<&Mode> {
        self.modes.iter().find(|m| m.code == self.round.mode)
    }

    /// The class for a client-chosen index, falling back to the first.
    #[must_use]
    pub fn class(&self, index: usize) -> (usize, &Class) {
        let i = if index < self.classes.len() { index } else { 0 };
        (i, &self.classes[i])
    }

    /// The class index with this name.
    #[must_use]
    pub fn class_named(&self, name: &str) -> Option<usize> {
        self.classes.iter().position(|c| c.name == name)
    }
}

/// The committed layers, for tests.
///
/// # Panics
/// If the committed rule files are inconsistent.
#[cfg(test)]
#[must_use]
pub fn committed() -> (Assumptions, Provenance) {
    let layers = [
        (
            "base".to_owned(),
            include_str!("../../data/rules/base.toml"),
        ),
        (
            "2016-08-06".to_owned(),
            include_str!("../../data/rules/2016-08-06.toml"),
        ),
    ]
    .map(|(l, t)| (l, t.to_owned()));
    Assumptions::from_layers(&layers).expect("committed rules are consistent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_layers_are_consistent() {
        let (a, prov) = committed();
        // Class order and loadouts must match the 2016-08-06 client.
        let names: Vec<_> = a.classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Triggerman",
                "Detective",
                "Hunter",
                "Run 'N Gun",
                "Vince",
                "Rocketeer",
                "Spray N' Pray",
                "Arsonist",
                "Duck"
            ]
        );
        assert_eq!(a.classes[0].weapons, [0, 5]);
        assert_eq!(a.weapons.len(), 10);
        assert!((a.weapons[0].dmg - 20.0).abs() < f64::EPSILON);
        // The dated layer replaces the provisional base.
        assert!((a.weapons[1].reload_speed - 1200.0).abs() < f64::EPSILON);
        let (_, o) = &prov.values["weapons.revolver.reloadSpeed"];
        assert_eq!(o.status, "INFERRED");
        let (_, o) = &prov.values["weapons.smg.bWidth"];
        assert_eq!(o.status, "PROVISIONAL");
        assert_eq!(a.mode().unwrap().code, "ffa");
        let snipe = a.modes.iter().find(|m| m.code == "snipe").unwrap();
        assert_eq!(snipe.forced_class.as_deref(), Some("Hunter"));
        assert_eq!(prov.hash.len(), 64);
    }

    #[test]
    fn every_value_has_a_known_status_and_source() {
        let (_, prov) = committed();
        for (path, (_, o)) in &prov.values {
            assert!(STATUSES.contains(&o.status.as_str()), "{path}");
            assert!(!o.source.is_empty(), "{path} has no source");
        }
    }

    #[test]
    fn later_layers_override_single_fields() {
        let base = r#"
[layer]
status = "ASSUMED"
source = "test base"
[[things]]
name = "a"
x = 1
y = 2
"#;
        let top = r#"
[layer]
status = "RECOVERED"
source = "test top"
[[things]]
name = "a"
y = 3
[[things]]
name = "b"
status = "INFERRED"
source = "elsewhere"
x = 4
"#;
        let (merged, prov) =
            merge_layers(&[("b".into(), base.into()), ("t".into(), top.into())]).unwrap();
        let things = merged["things"].as_array().unwrap();
        assert_eq!(things[0]["x"].as_integer(), Some(1));
        assert_eq!(things[0]["y"].as_integer(), Some(3));
        assert_eq!(things[1]["x"].as_integer(), Some(4));
        assert!(things[0].get("status").is_none());
        assert_eq!(prov.values["things.a.x"].1.status, "ASSUMED");
        assert_eq!(prov.values["things.a.y"].1.status, "RECOVERED");
        assert_eq!(prov.values["things.b.x"].1.source, "elsewhere");
    }

    #[test]
    fn rejects_unknown_status_and_headerless_layers() {
        let bad = "[layer]\nstatus = \"MAYBE\"\nsource = \"x\"\n";
        assert!(merge_layers(&[("x".into(), bad.into())]).is_err());
        assert!(merge_layers(&[("x".into(), "[world]\na = 1\n".into())]).is_err());
    }

    #[test]
    fn unknown_class_falls_back() {
        let (a, _) = committed();
        assert_eq!(a.class(99).0, 0);
        assert_eq!(a.class(2).1.name, "Hunter");
        assert_eq!(a.class_named("Rocketeer"), Some(5));
    }
}

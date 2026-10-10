//! Typed view of the rule layers in `data/rules/`.
//!
//! Server settings and game constants live here; class and weapon numbers
//! come from KRP's data and a balance preset (see [`super::data`]). The
//! numbers come from a stack of TOML layers: a provisional base, then
//! dated evidence, then optional local tuning. A
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
//! [[things]]                      # arrays of tables merge by `name`
//! name = "a"
//! status = "INFERRED"             # optional: for the fields of this entry
//! source = "elsewhere"
//! x = 1
//! ```
//!
//! The same `name` may appear more than once in one layer, so values with
//! different sources can sit in separate entries.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Deserialize;
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
    pub rules: Rules,
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
    /// Server ticks per second: projectiles, timers and pickups advance on
    /// this tick.
    pub update_hz: f64,
}

/// Game constants KRP's server hard-codes (`server/room.ts`, `game.ts`).
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Rules {
    pub max_players: usize,
    pub spawn_protection_ms: f64,
    pub explosive_clutter_hit_damage: f64,
    pub explosive_clutter_blast_radius: f64,
    pub duck_hit_damage: f64,
    pub duck_hit_radius: f64,
    pub lootcrate_points: u32,
    pub max_active_loot: usize,
    pub loot_interval_ms: f64,
    pub hardpoint_points: u32,
    pub hardpoint_interval_ms: f64,
    pub zone_war_points: u32,
    pub healthpack_heal: f64,
    pub healthpack_respawn_ms: f64,
    pub boss_kill_score: u32,
    pub kill_streak_window_ms: f64,
    pub round_end_countdown_s: u32,
    pub bullet_pool: usize,
    pub chat_max_len: usize,
    /// Sprays a player can have on the map at once (KRP: 1).
    pub sprays_per_player: u32,
    pub name_max_len: usize,
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
        if !(self.net.update_hz > 0.0 && self.net.update_hz <= 1000.0) {
            return Err(Error("net.update_hz must be in (0, 1000]".into()));
        }
        if self.rules.max_players == 0 || self.rules.bullet_pool == 0 {
            return Err(Error(
                "rules.max_players and rules.bullet_pool must be positive".into(),
            ));
        }
        if self.rules.sprays_per_player == 0 {
            return Err(Error("rules.sprays_per_player must be positive".into()));
        }
        Ok(())
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
            "recovered".to_owned(),
            include_str!("../../data/rules/recovered.toml"),
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
        assert!((a.world.tile_scale - 256.0).abs() < f64::EPSILON);
        assert_eq!(a.rules.max_players, 8);
        assert_eq!(prov.values["world.view_mult"].1.status, "RECOVERED");
        assert_eq!(prov.values["rules.duck_hit_damage"].1.status, "PROVISIONAL");
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
}

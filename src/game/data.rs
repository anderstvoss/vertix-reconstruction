//! Game data: KRP's classes, weapons, modes and cosmetics, with a balance
//! preset on top.
//!
//! `data/krp/` holds KRP's tables as JSON (see `scripts/import_krp.py`).
//! A balance preset from `data/balance/` then replaces class and weapon
//! numbers value by value, each with its basis and source, so the server
//! can run any version's balance ("balance version selection").

use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn read_json(path: &Path) -> Result<Value, Error> {
    let text =
        std::fs::read_to_string(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| Error(format!("{}: {e}", path.display())))
}

fn field<T: for<'de> Deserialize<'de>>(doc: &Value, key: &str, what: &str) -> Result<T, Error> {
    serde_json::from_value(doc.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| Error(format!("{what}.{key}: {e}")))
}

/// One character class.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Class {
    #[serde(rename = "classN", default)]
    pub name: Option<String>,
    pub max_health: f64,
    pub height: f64,
    pub width: f64,
    pub speed: f64,
    pub jump_strength: f64,
    pub gravity_strength: f64,
    pub weapon_indexes: Vec<usize>,
    /// False when the selected balance version predates the class.
    #[serde(default = "yes")]
    pub available: bool,
}

fn yes() -> bool {
    true
}

/// The numbers server logic needs from a weapon. The client gets the
/// whole weapon object ([`Weapon::json`]).
// Flags mirror KRP's object fields one for one.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponSpec {
    pub name: String,
    pub weapon_index: usize,
    pub dmg: f64,
    pub reload_speed: f64,
    pub bullets_per_shot: u32,
    pub spread: Vec<f64>,
    pub y_offset: f64,
    pub hold_dist: f64,
    pub b_dist: f64,
    pub b_speed: f64,
    pub b_width: f64,
    pub b_height: f64,
    #[serde(default)]
    pub b_rand_scale: Option<[f64; 2]>,
    pub c_acc: u32,
    #[serde(default)]
    pub max_life: Option<f64>,
    pub pierce: u32,
    pub bounce: bool,
    pub dist_based: bool,
    pub explode_on_death: bool,
    #[serde(default)]
    pub blast_radius: Option<f64>,
    #[serde(default)]
    pub self_damage: bool,
    pub b_sprite: u32,
}

#[derive(Debug, Clone)]
pub struct Weapon {
    pub spec: WeaponSpec,
    /// The weapon object as the client expects it.
    pub json: Map<String, Value>,
}

/// One game mode, in KRP's order (votes and rounds refer to the index).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub code: String,
    pub name: String,
    pub score: f64,
    pub desc1: String,
    pub desc2: String,
    pub teams: bool,
    #[serde(deserialize_with = "map_ids")]
    pub maps: Vec<String>,
    pub kill_score_mult: f64,
}

fn map_ids<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let ids: Vec<Value> = Vec::deserialize(d)?;
    Ok(ids
        .iter()
        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
        .collect())
}

impl Mode {
    /// `mapData.gameMode` as the client reads it.
    #[must_use]
    pub fn client_json(&self) -> Value {
        serde_json::json!({
            "code": self.code,
            "name": self.name,
            "score": self.score,
            "desc1": self.desc1,
            "desc2": self.desc2,
            "teams": self.teams,
            "killScoreMult": self.kill_score_mult,
        })
    }
}

/// Cosmetic catalogues as KRP sends them.
#[derive(Debug, Clone, Default)]
pub struct Cosmetics {
    pub hats: Vec<Value>,
    pub shirts: Vec<Value>,
    pub camos: Vec<Value>,
    pub sprays: Vec<Value>,
}

/// One value the balance preset set.
#[derive(Debug, Clone)]
pub struct BalanceValue {
    pub path: String,
    pub value: Value,
    pub basis: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Balance {
    pub id: String,
    pub title: String,
    pub values: Vec<BalanceValue>,
}

#[derive(Debug, Clone)]
pub struct GameData {
    pub classes: Vec<Class>,
    pub weapons: Vec<Weapon>,
    pub modes: Vec<Mode>,
    pub cosmetics: Cosmetics,
    pub balance: Balance,
    /// The KRP commit the tables came from.
    pub krp_commit: String,
}

impl GameData {
    /// Loads KRP's tables from `krp_dir` and the preset `balance` from
    /// `balance_dir`.
    ///
    /// # Errors
    /// Fails if a file is missing or malformed, or the preset names a class
    /// or weapon KRP does not have.
    pub fn load(krp_dir: &Path, balance_dir: &Path, balance: &str) -> Result<Self, Error> {
        Self::load_tuned(krp_dir, balance_dir, balance, &Value::Null)
    }

    /// As [`GameData::load`], with `tweaks` laid over the preset: the
    /// preset's own shape (`{"classes": {name: {stat: {"value": ..}}},
    /// "weapons": {..}}`), as the admin console's `tune` builds it.
    ///
    /// # Errors
    /// As [`GameData::load`].
    pub fn load_tuned(
        krp_dir: &Path,
        balance_dir: &Path,
        balance: &str,
        tweaks: &Value,
    ) -> Result<Self, Error> {
        if balance.is_empty()
            || !balance
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return Err(Error(format!("bad balance preset name {balance:?}")));
        }
        let mut preset = read_json(&balance_dir.join(format!("{balance}.json")))?;
        merge(&mut preset, tweaks);
        Self::from_docs(
            &read_json(&krp_dir.join("loadouts.json"))?,
            &read_json(&krp_dir.join("gamemodes.json"))?,
            &read_json(&krp_dir.join("skins.json"))?,
            &read_json(&krp_dir.join("sprays.json"))?,
            &preset,
        )
    }

    /// Builds the data from parsed documents.
    ///
    /// # Errors
    /// As [`GameData::load`].
    pub fn from_docs(
        loadouts: &Value,
        modes: &Value,
        skins: &Value,
        sprays: &Value,
        preset: &Value,
    ) -> Result<Self, Error> {
        let mut classes: Vec<Map<String, Value>> = field(loadouts, "classes", "loadouts")?;
        let mut weapons: Vec<Map<String, Value>> = field(loadouts, "weapons", "loadouts")?;
        let mut values = Vec::new();

        // Preset classes are keyed by `classN` in KRP's order; KRP's last
        // class has no `classN`, so match by position and check names.
        let preset_classes = preset
            .get("classes")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (i, (key, stats)) in preset_classes.iter().enumerate() {
            let at = classes
                .iter()
                .position(|c| c.get("classN").and_then(Value::as_str) == Some(key.as_str()))
                .or_else(|| (i < classes.len() && classes[i].get("classN").is_none()).then_some(i))
                .ok_or_else(|| Error(format!("balance preset names unknown class {key}")))?;
            overlay(
                &mut classes[at],
                stats,
                &format!("classes.{key}"),
                &mut values,
            );
        }
        let preset_weapons = preset
            .get("weapons")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (key, stats) in &preset_weapons {
            let at = weapons
                .iter()
                .position(|w| w.get("name").and_then(Value::as_str) == Some(key.as_str()))
                .ok_or_else(|| Error(format!("balance preset names unknown weapon {key}")))?;
            overlay(
                &mut weapons[at],
                stats,
                &format!("weapons.{key}"),
                &mut values,
            );
        }

        let classes = classes
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                serde_json::from_value(Value::Object(c))
                    .map_err(|e| Error(format!("class {i}: {e}")))
            })
            .collect::<Result<Vec<Class>, _>>()?;
        let weapons = weapons
            .into_iter()
            .enumerate()
            .map(|(i, json)| {
                let spec: WeaponSpec = serde_json::from_value(Value::Object(json.clone()))
                    .map_err(|e| Error(format!("weapon {i}: {e}")))?;
                if spec.spread.is_empty() {
                    return Err(Error(format!("weapon {} has no spread", spec.name)));
                }
                Ok(Weapon { spec, json })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for c in &classes {
            if let Some(&bad) = c.weapon_indexes.iter().find(|&&w| w >= weapons.len()) {
                return Err(Error(format!("a class uses unknown weapon {bad}")));
            }
        }
        let modes: Vec<Mode> = field(modes, "modes", "gamemodes")?;
        if modes.is_empty() {
            return Err(Error("no modes".into()));
        }
        let cosmetics = Cosmetics {
            hats: field(skins, "hats", "skins")?,
            shirts: field(skins, "shirts", "skins")?,
            camos: field(skins, "camos", "skins")?,
            sprays: field(sprays, "sprays", "sprays")?,
        };
        let krp_commit = loadouts
            .pointer("/source/commit")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let balance = Balance {
            id: preset
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_owned(),
            title: preset
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            values,
        };
        Ok(Self {
            classes,
            weapons,
            modes,
            cosmetics,
            balance,
            krp_commit,
        })
    }

    /// The class for a client-chosen index, falling back to the first.
    #[must_use]
    pub fn class(&self, index: usize) -> (usize, &Class) {
        let i = if index < self.classes.len() && self.classes[index].available {
            index
        } else {
            0
        };
        (i, &self.classes[i])
    }

    /// The index of the mode with this code.
    #[must_use]
    pub fn mode_index(&self, code: &str) -> Option<usize> {
        self.modes.iter().position(|m| m.code == code)
    }

    /// One line per preset value, for `--explain-rules`.
    #[must_use]
    pub fn explain(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for v in &self.balance.values {
            let _ = writeln!(
                out,
                "balance.{} = {}\t{}\t{}\t[{}]",
                v.path, v.value, v.basis, v.source, self.balance.id
            );
        }
        out
    }
}

/// Applies a preset entry's `{stat: {value, basis, source}}` to an object.
/// Lays `over` onto `base`, object by object; other values replace.
fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge(slot, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (_, Value::Null) => {}
        (b, o) => *b = o.clone(),
    }
}

fn overlay(
    target: &mut Map<String, Value>,
    stats: &Value,
    path: &str,
    out: &mut Vec<BalanceValue>,
) {
    let Some(stats) = stats.as_object() else {
        return;
    };
    for (stat, entry) in stats {
        match entry {
            Value::Object(e) if e.contains_key("value") => {
                let value = e["value"].clone();
                target.insert(stat.clone(), value.clone());
                let text = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
                out.push(BalanceValue {
                    path: format!("{path}.{stat}"),
                    value,
                    basis: text("basis"),
                    source: text("source"),
                });
            }
            Value::Bool(b) if stat == "available" => {
                target.insert(stat.clone(), Value::Bool(*b));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
pub(crate) fn committed(balance: &str) -> GameData {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    GameData::load(&root.join("krp"), &root.join("balance"), balance).expect("committed data loads")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn krp_tables_load_with_every_preset() {
        let index =
            read_json(&Path::new(env!("CARGO_MANIFEST_DIR")).join("data/balance/index.json"))
                .unwrap();
        let ids: Vec<&str> = index["presets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert!(ids.len() >= 60);
        for id in ids {
            let d = committed(id);
            assert_eq!(d.classes.len(), 12, "{id}");
            assert_eq!(d.weapons.len(), 13, "{id}");
            assert!(!d.balance.values.is_empty(), "{id}");
        }
    }

    #[test]
    fn presets_change_numbers_and_record_sources() {
        let krp = committed("krp");
        let best = committed("best");
        let codes: Vec<&str> = krp.modes.iter().map(|m| m.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "ffa", "tdm", "hp", "lc", "snipe", "boss", "zmtch", "rckt", "pyro"
            ]
        );
        assert_eq!(krp.class(2).1.name.as_deref(), Some("Hunter"));
        assert_eq!(krp.modes[0].maps[1], "5");
        // Every preset value carries a basis and a source.
        for v in &best.balance.values {
            assert!(!v.basis.is_empty() && !v.source.is_empty(), "{}", v.path);
        }
        // The client sees the preset number in the weapon object too.
        for w in &best.weapons {
            assert_eq!(w.json["dmg"].as_f64(), Some(w.spec.dmg));
        }
        // An early version hides classes that did not exist yet.
        let early = committed("v1.0");
        assert!(!early.classes[5].available);
        assert_eq!(early.class(5).0, 0);
    }

    #[test]
    fn rejects_bad_preset_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        for bad in ["../x", "", "a/b"] {
            assert!(GameData::load(&root.join("krp"), &root.join("balance"), bad).is_err());
        }
    }
}

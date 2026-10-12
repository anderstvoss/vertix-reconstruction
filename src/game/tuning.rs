//! The admin console's version string and balance controls: `version`,
//! `balance` and `tune`.
//!
//! `tune` keeps its values in a preset's own shape and reloads the game
//! data with them laid over the current preset, so a tuned value goes
//! through exactly the path a preset value does, and switching preset
//! keeps the tuning on top.

use serde_json::{Map, Value, json};

use super::Game;
use super::data::{Class, GameData, Weapon};
use crate::admin::Reply;

type Res = Result<Reply, String>;

/// Class values `tune` can set (KRP's field names).
const CLASS_FIELDS: &[&str] = &[
    "maxHealth",
    "speed",
    "jumpStrength",
    "gravityStrength",
    "height",
    "width",
];

/// A class's tunable values.
#[must_use]
pub fn class_fields(c: &Class) -> Value {
    json!({
        "maxHealth": c.max_health,
        "speed": c.speed,
        "jumpStrength": c.jump_strength,
        "gravityStrength": c.gravity_strength,
        "height": c.height,
        "width": c.width,
    })
}

/// A weapon's tunable values: every number and flag the client gets.
#[must_use]
pub fn weapon_fields(w: &Weapon) -> Value {
    Value::Object(
        w.json
            .iter()
            .filter(|(_, v)| v.is_number() || v.is_boolean())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Map<_, _>>(),
    )
}

impl Game {
    /// Loads preset `id` with the tuning on top, if it keeps the tables'
    /// shape (players hold indexes into them).
    fn reload_data(&self, id: &str, tweaks: &Value) -> Result<GameData, String> {
        let p = &self.admin.paths;
        let mut data = GameData::load_tuned(&p.krp_data, &p.balance_dir, id, tweaks)
            .map_err(|e| e.to_string())?;
        // Sprays added from the sprays folder are not in the preset.
        data.cosmetics
            .sprays
            .clone_from(&self.data.cosmetics.sprays);
        if data.modes.len() != self.data.modes.len()
            || data.classes.len() != self.data.classes.len()
            || data.weapons.len() != self.data.weapons.len()
        {
            return Err("that changes the tables' shape; restart the server with it".into());
        }
        Ok(data)
    }

    pub(super) fn switch_balance(&mut self, id: &str) -> Res {
        self.data = self.reload_data(id, &self.admin.tweaks)?;
        let tuned = self.tweak_list().len();
        Ok(Reply::ok(format!(
            "balance preset {}{} from each player's next spawn",
            self.data.balance.id,
            if tuned > 0 {
                format!(" with {tuned} tuned values")
            } else {
                String::new()
            }
        )))
    }

    /// The game versions the research has a balance sheet for, oldest
    /// first, with the date each was evaluated at.
    pub(super) fn versions(&self) -> Vec<Value> {
        let dir = &self.admin.paths.balance_dir;
        let index: Value = std::fs::read_to_string(dir.join("index.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let presets = index["presets"].as_array().cloned().unwrap_or_default();
        presets
            .iter()
            .filter(|p| p["kind"] == "version")
            .filter_map(|p| {
                let id = p["id"].as_str()?;
                let date = std::fs::read_to_string(dir.join(format!("{id}.json")))
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .and_then(|v| v["evaluated_at"].as_str().map(str::to_owned));
                Some(json!({"id": id, "string": version_string(id), "date": date}))
            })
            .collect()
    }

    /// Tells every room the version string changed.
    fn announce_version(&mut self) {
        let msg = format!(
            "Server version is now {} (menus show it after a reload)",
            self.admin.version.get()
        );
        for s in &mut self.rooms {
            s.room.admin_say(&msg);
        }
    }

    /// `version`, `version set <text>`, `version use <id> [label]`,
    /// `version reset`.
    pub(super) fn version_command(&mut self, a: &[String]) -> Res {
        let v = self.admin.version.clone();
        match a.first().map(String::as_str) {
            None => Ok(
                Reply::ok(format!("version {} (menu label {:?})", v.get(), v.label()))
                    .with_data(json!({"version": v.get(), "versions": self.versions()})),
            ),
            Some("set") => {
                let text = a[1..].join(" ");
                v.set(&text)?;
                self.announce_version();
                Ok(Reply::ok(format!("version string {text}")))
            }
            Some("reset") => {
                v.set(&crate::version::default_string())?;
                self.announce_version();
                Ok(Reply::ok(format!("version string {}", v.get())))
            }
            Some("use") => {
                let id = a
                    .get(1)
                    .ok_or("which version? `list versions` lists them")?;
                let label_only = a.get(2).map(String::as_str) == Some("label");
                let known = self
                    .versions()
                    .iter()
                    .any(|x| x["id"].as_str() == Some(id.as_str()));
                if !known {
                    return Err(format!("no researched version {id:?}; `list versions`"));
                }
                v.set(&version_string(id))?;
                let mut text = format!("version string {}", v.get());
                if !label_only {
                    let r = self.switch_balance(id)?;
                    text = format!("{text}, {}", r.text);
                }
                self.announce_version();
                Ok(Reply::ok(text))
            }
            Some(_) => Err("version [set <text> | use <version> [label] | reset]".into()),
        }
    }

    /// Every tuned value as `classes.Name.field = value`.
    pub(super) fn tweak_list(&self) -> Vec<String> {
        let mut out = Vec::new();
        for kind in ["classes", "weapons"] {
            let Some(things) = self.admin.tweaks[kind].as_object() else {
                continue;
            };
            for (name, fields) in things {
                for (f, e) in fields.as_object().into_iter().flatten() {
                    out.push(format!("{kind}.{name}.{f} = {}", e["value"]));
                }
            }
        }
        out
    }

    /// `tune`, `tune reset`, `tune show <class|weapon> <name>`,
    /// `tune <class|weapon> <name> <field> <value>`.
    pub(super) fn tune(&mut self, a: &[String]) -> Res {
        match a.first().map(String::as_str) {
            None | Some("list") => {
                let list = self.tweak_list();
                let text = if list.is_empty() {
                    "nothing tuned".to_owned()
                } else {
                    list.join("\n")
                };
                Ok(Reply::ok(text).with_data(json!(list)))
            }
            Some("reset") => {
                let id = self.data.balance.id.clone();
                self.data = self.reload_data(&id, &Value::Null)?;
                self.admin.tweaks = Value::Null;
                Ok(Reply::ok(format!("tuning cleared; balance preset {id}")))
            }
            Some("show") => {
                let (kind, i) = self.tune_target(a.get(1), a.get(2))?;
                let fields = if kind == "classes" {
                    class_fields(&self.data.classes[i])
                } else {
                    weapon_fields(&self.data.weapons[i])
                };
                let text = fields
                    .as_object()
                    .map(|o| {
                        o.iter()
                            .map(|(k, v)| format!("{k} = {v}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                Ok(Reply::ok(text).with_data(fields))
            }
            Some("class" | "weapon") => {
                let (kind, i) = self.tune_target(a.first(), a.get(1))?;
                let field = a.get(2).ok_or("which value?")?;
                let raw = a.get(3).ok_or("set it to what?")?;
                let value: Value =
                    serde_json::from_str(raw).map_err(|_| format!("{raw:?} is not a number"))?;
                let (name, current) = if kind == "classes" {
                    let c = &self.data.classes[i];
                    if !CLASS_FIELDS.contains(&field.as_str()) {
                        return Err(format!("classes tune {}", CLASS_FIELDS.join(", ")));
                    }
                    (c.name.clone().unwrap_or_default(), class_fields(c)[field].clone())
                } else {
                    let w = &self.data.weapons[i];
                    (w.spec.name.clone(), weapon_fields(w)[field].clone())
                };
                if current.is_null() {
                    return Err(format!("{name} has no tunable {field:?}; `tune show`"));
                }
                if current.is_boolean() != value.is_boolean() || current.is_number() != value.is_number() {
                    return Err(format!("{field} is {current}; give the same kind of value"));
                }
                let mut tweaks = self.admin.tweaks.clone();
                if !tweaks.is_object() {
                    tweaks = json!({});
                }
                tweaks[kind][&name][field] = json!({
                    "value": value,
                    "basis": "admin",
                    "source": "admin console tune",
                });
                let id = self.data.balance.id.clone();
                self.data = self.reload_data(&id, &tweaks)?;
                self.admin.tweaks = tweaks;
                Ok(Reply::ok(format!(
                    "{name} {field} = {value} (was {current}) from each player's next spawn"
                )))
            }
            Some(_) => Err(
                "tune [list | reset | show <class|weapon> <name> | <class|weapon> <name> <field> <value>]"
                    .into(),
            ),
        }
    }

    /// A class or weapon by index or name (ignoring case).
    fn tune_target(
        &self,
        kind: Option<&String>,
        spec: Option<&String>,
    ) -> Result<(&'static str, usize), String> {
        let spec = spec.ok_or("which one? (index or name)")?;
        let low = spec.to_lowercase();
        let idx = spec.parse::<usize>().ok();
        match kind.map(String::as_str) {
            Some("class") => self
                .data
                .classes
                .iter()
                .enumerate()
                .position(|(i, c)| {
                    idx == Some(i) || c.name.as_deref().map(str::to_lowercase) == Some(low.clone())
                })
                .map(|i| ("classes", i))
                .ok_or_else(|| format!("no class {spec:?}; `list classes`")),
            Some("weapon") => self
                .data
                .weapons
                .iter()
                .enumerate()
                .position(|(i, w)| idx == Some(i) || w.spec.name.to_lowercase() == low)
                .map(|i| ("weapons", i))
                .ok_or_else(|| format!("no weapon {spec:?}; `list weapons`")),
            _ => Err("class or weapon?".into()),
        }
    }
}

/// The string a researched version is shown as: `v3.8` -> `V3.8`.
fn version_string(id: &str) -> String {
    let mut s = id.to_owned();
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}

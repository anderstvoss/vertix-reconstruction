//! The objects the server sends, as KRP's client keeps them (`types.ts`).

#![forbid(unsafe_code)]

use serde::Deserialize;
use serde_json::Value;
use vertix_sim::data::WeaponSpec;

/// A hat or shirt as worn (`account.hat`, `account.shirt`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Wearable {
    pub id: i64,
    pub left: bool,
    pub up: bool,
    pub name_y: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Account {
    pub clan: String,
    pub rank: f64,
    pub hat: Option<Wearable>,
    pub shirt: Option<Wearable>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SprayInfo {
    pub scale: f64,
    pub alpha: f64,
    pub resolution: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Spray {
    pub src: String,
    pub info: SprayInfo,
}

/// A weapon: KRP's weapon object, with the bullet numbers the shared
/// [`WeaponSpec`] reads and the client's own state.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Weapon {
    #[serde(flatten)]
    pub spec: WeaponSpec,
    #[serde(default)]
    pub ammo: f64,
    #[serde(default)]
    pub max_ammo: f64,
    #[serde(default)]
    pub fire_rate: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub length: f64,
    #[serde(default)]
    pub b_trail: f64,
    #[serde(default)]
    pub glow_width: Option<f64>,
    #[serde(default)]
    pub glow_height: Option<f64>,
    #[serde(default)]
    pub shake: f64,
    #[serde(default)]
    pub reload_time: f64,
    #[serde(default)]
    pub spread_index: usize,
    #[serde(default)]
    pub last_shot: f64,
    /// Camo index, -1 for none.
    #[serde(default = "no_camo")]
    pub camo: f64,
    /// Drawn in front of the body (KRP `front`), set every frame.
    #[serde(skip)]
    pub front: bool,
}

fn no_camo() -> f64 {
    -1.0
}

/// A player as KRP's client holds them.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Player {
    pub id: Value,
    pub index: u32,
    pub name: String,
    pub team: String,
    pub class_index: usize,
    pub current_weapon: usize,
    pub weapons: Vec<Weapon>,
    pub health: f64,
    pub max_health: f64,
    pub height: f64,
    pub width: f64,
    pub speed: f64,
    pub jump_y: f64,
    pub jump_delta: f64,
    pub jump_strength: f64,
    pub gravity_strength: f64,
    pub jump_countdown: f64,
    pub frame_countdown: f64,
    pub kills: f64,
    pub deaths: f64,
    pub score: f64,
    pub angle: f64,
    pub x: f64,
    pub y: f64,
    pub old_x: f64,
    pub old_y: f64,
    pub total_damage: f64,
    pub total_healing: f64,
    pub total_goals: f64,
    pub is_spawn_protected: bool,
    pub name_y_offset: f64,
    pub dead: bool,
    pub on_screen: bool,
    pub anim_index: i64,
    pub is_boss: bool,
    pub spray: Option<Spray>,
    pub liked_by: Value,
    pub account: Account,
    pub logged_in: bool,
    /// Last input the server applied (own player only).
    pub isn: f64,
    #[serde(skip)]
    pub x_speed: f64,
    #[serde(skip)]
    pub y_speed: f64,
    #[serde(skip)]
    pub hit_flash: f64,
}

impl Player {
    /// KRP `getCurrentWeapon`.
    #[must_use]
    pub fn weapon(&self) -> Option<&Weapon> {
        self.weapons.get(self.current_weapon)
    }

    pub fn weapon_mut(&mut self) -> Option<&mut Weapon> {
        self.weapons.get_mut(self.current_weapon)
    }
}

/// `mapData.gameMode`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameMode {
    pub code: String,
    pub name: String,
    pub score: f64,
    pub desc1: String,
    pub desc2: String,
    pub teams: bool,
    pub kill_score_mult: f64,
}

impl GameMode {
    /// The shared crate's mode, for laying out tiles.
    #[must_use]
    pub fn sim(&self) -> vertix_sim::data::Mode {
        vertix_sim::data::Mode {
            code: self.code.clone(),
            name: self.name.clone(),
            score: self.score,
            desc1: self.desc1.clone(),
            desc2: self.desc2.clone(),
            teams: self.teams,
            maps: Vec::new(),
            kill_score_mult: self.kill_score_mult,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pickup {
    pub x: f64,
    pub y: f64,
    pub active: bool,
    pub scale: f64,
    #[serde(rename = "type")]
    pub kind: String,
}

/// KRP's flag object (hardpoint flags).
#[derive(Debug, Clone)]
pub struct Flag {
    pub team: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub ai: usize,
    pub ac: i32,
}

/// Reads a number the way KRP's loose JavaScript would (missing is 0).
#[must_use]
pub fn num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_player_objects_parse() {
        let w = &vertix_sim::data::committed("krp").weapons[0].json;
        let mut weapon = w.clone();
        weapon.insert("spreadIndex".into(), serde_json::json!(0));
        let p: Player = serde_json::from_value(serde_json::json!({
            "index": 3, "name": "a", "team": "red", "classIndex": 0,
            "weapons": [weapon], "account": {"clan": "", "rank": 0},
            "spray": null, "x": 10.5, "dead": false,
        }))
        .unwrap();
        assert_eq!(p.index, 3);
        assert_eq!(p.weapon().unwrap().spec.name, "smg");
        assert!((p.weapon().unwrap().camo + 1.0).abs() < 1e-9);
    }
}

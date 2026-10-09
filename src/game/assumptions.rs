//! Typed view of `data/assumptions.toml`.
//!
//! Game logic reads numbers only from here, never from literals, so every
//! assumption can be found, sourced and changed in one file.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct Assumptions {
    pub world: World,
    pub net: Net,
    pub mode: Mode,
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
    pub tick_ms: u64,
    pub max_input_delta_ms: f64,
}

/// Sent to the client as `mapData.gameMode`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Mode {
    pub name: String,
    pub code: String,
    pub desc1: String,
    pub desc2: String,
    pub score: u32,
    pub teams: bool,
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

impl Assumptions {
    /// Parses and checks an assumptions file.
    ///
    /// # Errors
    /// Fails if the file is unreadable, malformed, or inconsistent.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text =
            std::fs::read_to_string(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        Self::parse(&text)
    }

    /// Parses and checks assumptions from TOML text.
    ///
    /// # Errors
    /// Fails if the text is malformed or inconsistent.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let a: Self = toml::from_str(text).map_err(|e| Error(e.to_string()))?;
        a.validate()?;
        Ok(a)
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
        if self.net.tick_ms == 0 {
            return Err(Error("net.tick_ms must be positive".into()));
        }
        Ok(())
    }

    /// The class for a client-chosen index, falling back to the first.
    #[must_use]
    pub fn class(&self, index: usize) -> (usize, &Class) {
        let i = if index < self.classes.len() { index } else { 0 };
        (i, &self.classes[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed() -> Assumptions {
        Assumptions::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data/assumptions.toml"
        )))
        .unwrap()
    }

    #[test]
    fn committed_file_is_consistent() {
        let a = committed();
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
        // The one recovered weapon number for the target date.
        assert!((a.weapons[0].dmg - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_class_falls_back() {
        let a = committed();
        assert_eq!(a.class(99).0, 0);
        assert_eq!(a.class(2).1.name, "Hunter");
    }
}

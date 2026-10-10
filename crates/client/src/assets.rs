//! The sprite pack (`res.zip`) and the sprites KRP's client picks from it
//! (`loadPlayerSprites`, `loadDefaultSprites`).

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read};

use macroquad::prelude::*;

use crate::gfx::Image;

/// Every PNG in the pack, by its path inside the zip (`sprites/...png`).
pub struct Pack {
    files: HashMap<String, Texture2D>,
}

impl Pack {
    /// Reads `res.zip` (or a mod zip, whose paths start `vertixmod/`).
    ///
    /// # Errors
    /// Fails if the zip cannot be read.
    pub fn from_zip(bytes: &[u8]) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let mut files = HashMap::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace("vertixmod/", "");
            if !name.starts_with("sprites/")
                || !std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"))
            {
                continue;
            }
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
            if let Ok(img) =
                macroquad::texture::Image::from_file_with_format(&data, Some(ImageFormat::Png))
            {
                let tex = Texture2D::from_image(&img);
                tex.set_filter(FilterMode::Nearest);
                files.insert(name, tex);
            }
        }
        Ok(Self { files })
    }

    /// KRP `getSprite(name)`: `name.png`, if the pack has it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Image> {
        self.files
            .get(&format!("{name}.png"))
            .cloned()
            .map(Image::sprite)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// One class's sprites. Index 0 is the upper body, 1..=3 the walk frames.
pub struct ClassSprites {
    pub up: Vec<Option<Image>>,
    pub down: Vec<Option<Image>>,
    pub left: Vec<Option<Image>>,
    pub right: Vec<Option<Image>>,
    pub arm: Option<Image>,
    pub head_down: Option<Image>,
    pub head_up: Option<Image>,
    pub head_left: Option<Image>,
    pub head_right: Option<Image>,
}

pub struct WeaponSprites {
    pub up: Option<Image>,
    pub down: Option<Image>,
    pub left: Option<Image>,
    pub right: Option<Image>,
    pub icon: Option<Image>,
}

/// The sprites the game draws, picked as KRP's client picks them.
pub struct Sprites {
    pub classes: Vec<ClassSprites>,
    pub flags: Vec<Option<Image>>,
    pub clutter: Vec<Option<Image>>,
    pub wall: Option<Image>,
    pub ambient: Vec<Option<Image>>,
    pub dark_filler: Option<Image>,
    pub light: Option<Image>,
    pub floors: Vec<Option<Image>>,
    pub sidewalk: Option<Image>,
    pub wall_segments: Vec<Option<Image>>,
    pub particles: Vec<Option<Image>>,
    pub healthpack: Option<Image>,
    pub lootcrate: Option<Image>,
    pub weapons: Vec<WeaponSprites>,
    pub bullets: Vec<Option<Image>>,
}

/// A class as the menu and sprite loader need it (KRP `characterClasses`).
#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: String,
    pub folder: String,
    pub has_down: bool,
    pub primary: String,
    pub secondary: String,
}

/// KRP's classes and weapon names from the committed tables
/// (`data/krp/loadouts.json`).
#[must_use]
pub fn krp_loadouts() -> (Vec<ClassInfo>, Vec<String>) {
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../../../data/krp/loadouts.json")).unwrap_or_default();
    let s = |v: &serde_json::Value, k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let classes = doc
        .get("classes")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| ClassInfo {
                    name: c
                        .get("classN")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("???")
                        .to_owned(),
                    folder: s(c, "folderName"),
                    has_down: c
                        .get("hasDown")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    primary: s(c, "pWeapon"),
                    secondary: s(c, "sWeapon"),
                })
                .collect()
        })
        .unwrap_or_default();
    let weapons = doc
        .get("weapons")
        .and_then(serde_json::Value::as_array)
        .map(|a| a.iter().map(|w| s(w, "name")).collect())
        .unwrap_or_default();
    (classes, weapons)
}

impl Sprites {
    /// `loadPlayerSprites` + `loadDefaultSprites` with base `sprites/`.
    #[must_use]
    pub fn pick(pack: &Pack, classes: &[ClassInfo], weapon_names: &[String]) -> Self {
        let g = |n: &str| pack.get(&format!("sprites/{n}"));
        let mut class_sprites = Vec::new();
        for c in classes {
            let f = &c.folder;
            let mut up = vec![g(&format!("characters/{f}/up"))];
            let mut down = vec![g(&format!("characters/{f}/down"))];
            let mut left = vec![g(&format!("characters/{f}/left"))];
            let mut right = vec![g(&format!("characters/{f}/left")).map(|i| i.flipped())];
            for i in 0..3 {
                up.push(g(&format!("characters/{f}/up{}", i + 1)));
                down.push(if c.has_down {
                    g(&format!("characters/{f}/down{}", i + 1))
                } else {
                    g(&format!("characters/{f}/up{}", i + 1))
                });
                // KRP's walk cycle for the sides reuses frame 1 as frame 3.
                let side = if i >= 2 { 0 } else { i };
                left.push(g(&format!("characters/{f}/left{}", side + 1)));
                right.push(g(&format!("characters/{f}/left{}", side + 1)).map(|i| i.flipped()));
            }
            class_sprites.push(ClassSprites {
                up,
                down,
                left,
                right,
                arm: g(&format!("characters/{f}/arm")),
                head_down: g(&format!("characters/{f}/hd")),
                head_up: g(&format!("characters/{f}/hu")),
                head_left: g(&format!("characters/{f}/hl")),
                head_right: g(&format!("characters/{f}/hl")).map(|i| i.flipped()),
            });
        }
        let weapons = weapon_names
            .iter()
            .map(|n| WeaponSprites {
                up: g(&format!("weapons/{n}/up")),
                down: g(&format!("weapons/{n}/up")),
                left: g(&format!("weapons/{n}/left")),
                right: g(&format!("weapons/{n}/left")).map(|i| i.flipped()),
                icon: g(&format!("weapons/{n}/icon")),
            })
            .collect();
        Self {
            classes: class_sprites,
            flags: ["flagb1", "flagb2", "flagb3", "flagr1", "flagr2", "flagr3"]
                .iter()
                .map(|n| g(&format!("flags/{n}")))
                .collect(),
            clutter: ["crate1", "barrel1", "barrel2", "bottle1", "spike1"]
                .iter()
                .map(|n| g(&format!("clutter/{n}")))
                .collect(),
            wall: g("wall1"),
            ambient: vec![g("ambient1")],
            dark_filler: g("darkfiller"),
            light: g("lighting"),
            floors: vec![g("ground1"), g("ground2"), g("ground3")],
            sidewalk: g("sidewalk1"),
            wall_segments: vec![g("wallSegment1"), g("wallSegment2"), g("wallSegment3")],
            particles: vec![
                g("particles/blood/blood"),
                g("particles/oil/oil"),
                g("particles/wall"),
                g("particles/hole"),
                g("particles/blood/splatter1"),
                g("particles/blood/splatter2"),
                g("particles/explosion"),
            ],
            healthpack: g("healthpack"),
            lootcrate: g("lootCrate1"),
            weapons,
            bullets: vec![
                g("weapons/bullet"),
                g("weapons/grenade"),
                g("weapons/flame"),
            ],
        }
    }
}

/// Loads a PNG downloaded at run time (hats, shirts, camos, sprays).
#[must_use]
pub fn texture_from_png(bytes: &[u8]) -> Option<Texture2D> {
    let img =
        macroquad::texture::Image::from_file_with_format(bytes, Some(ImageFormat::Png)).ok()?;
    let tex = Texture2D::from_image(&img);
    tex.set_filter(FilterMode::Nearest);
    Some(tex)
}

#[cfg(test)]
mod tests {
    #[test]
    fn krp_tables_list_classes_and_weapons() {
        let (classes, weapons) = super::krp_loadouts();
        assert!(classes.len() >= 9);
        assert_eq!(classes[0].folder, "triggerman");
        assert!(weapons.iter().any(|w| w == "smg"));
    }
}

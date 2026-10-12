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

/// Spray ids and names from a sprays list: KRP's table
/// (`data/krp/sprays.json`) or the server's `/sprays/index.json`, which
/// adds the sprays in its sprays folder.
#[must_use]
pub fn spray_list(json: &[u8]) -> Vec<(u32, String)> {
    let doc: serde_json::Value = serde_json::from_slice(json).unwrap_or_default();
    doc.get("sprays")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let id = u32::try_from(s.get("id")?.as_u64()?).ok()?;
                    let name = s.get("name").and_then(serde_json::Value::as_str);
                    Some((id, name.unwrap_or("").to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// KRP's sprays (`data/krp/sprays.json`).
#[must_use]
pub fn krp_sprays() -> Vec<(u32, String)> {
    spray_list(include_bytes!("../../../data/krp/sprays.json"))
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

/// The server's mod pack holding the 2016 sprites (`/mods/<key>/vertixmod.zip`).
pub const FLOOR_2016_PACK: &str = "original-2016";

impl Sprites {
    /// Takes the ground tiles from `pack` (the rest stays as picked).
    /// Returns whether it had all three.
    pub fn use_floors_from(&mut self, pack: &Pack) -> bool {
        let floors: Vec<Option<Image>> = (1..=3)
            .map(|i| pack.get(&format!("sprites/ground{i}")))
            .collect();
        if floors.iter().all(Option::is_some) {
            self.floors = floors;
            true
        } else {
            false
        }
    }

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

/// Decodes a PNG downloaded at run time.
#[must_use]
pub fn image_from_png(bytes: &[u8]) -> Option<macroquad::texture::Image> {
    macroquad::texture::Image::from_file_with_format(bytes, Some(ImageFormat::Png)).ok()
}

/// A texture with hard pixels.
#[must_use]
pub fn texture_from_image(img: &macroquad::texture::Image) -> Texture2D {
    let tex = Texture2D::from_image(img);
    tex.set_filter(FilterMode::Nearest);
    tex
}

/// The size a spray is drawn at and the size it is shrunk to first, as the
/// 2016 client and KRP size them: shrunk to `resolution` (30 px) and blown
/// up to `scale` (64 px) with hard pixels. The one change: an image that is
/// not square keeps its shape inside that square.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn spray_sizes(w: u16, h: u16, resolution: f64, scale: f64) -> ((u16, u16), (f32, f32)) {
    let (w, h) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let k = w.max(h);
    let res = if resolution > 0.0 { resolution } else { 30.0 };
    let scale = if scale > 0.0 { scale } else { 64.0 };
    let small = (
        (res * w / k).round().max(1.0) as u16,
        (res * h / k).round().max(1.0) as u16,
    );
    let shown = (
        (scale * w / k).round().max(1.0) as f32,
        (scale * h / k).round().max(1.0) as f32,
    );
    (small, shown)
}

/// Shrinks an image to `w` x `h` by averaging the pixels each new pixel
/// covers (alpha-weighted, as a canvas does), for a spray's resolution.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn shrink(img: &macroquad::texture::Image, w: u16, h: u16) -> macroquad::texture::Image {
    let (sw, sh) = (usize::from(img.width), usize::from(img.height));
    let (dw, dh) = (usize::from(w.max(1)), usize::from(h.max(1)));
    let mut out = vec![0u8; dw * dh * 4];
    for dy in 0..dh {
        let (y0, y1) = (dy * sh / dh, ((dy + 1) * sh / dh).max(dy * sh / dh + 1));
        for dx in 0..dw {
            let (x0, x1) = (dx * sw / dw, ((dx + 1) * sw / dw).max(dx * sw / dw + 1));
            let mut acc = [0f64; 4];
            let mut n = 0f64;
            for y in y0..y1.min(sh) {
                for x in x0..x1.min(sw) {
                    let p = &img.bytes[(y * sw + x) * 4..(y * sw + x) * 4 + 4];
                    let a = f64::from(p[3]);
                    for c in 0..3 {
                        acc[c] += f64::from(p[c]) * a;
                    }
                    acc[3] += a;
                    n += 1.0;
                }
            }
            let o = &mut out[(dy * dw + dx) * 4..(dy * dw + dx) * 4 + 4];
            if acc[3] > 0.0 {
                for c in 0..3 {
                    o[c] = (acc[c] / acc[3]).round() as u8;
                }
                o[3] = (acc[3] / n.max(1.0)).round() as u8;
            }
        }
    }
    macroquad::texture::Image {
        bytes: out,
        width: w.max(1),
        height: h.max(1),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spray_lists_and_sizes() {
        let krp = super::krp_sprays();
        assert_eq!(krp.first(), Some(&(1, "Strike".to_owned())));
        let extra = super::spray_list(br#"{"sprays":[{"id":84,"name":"Cat","info":{}}]}"#);
        assert_eq!(extra, vec![(84, "Cat".to_owned())]);
        // Square: 30 px, drawn 64 px. Wide: keeps its shape.
        assert_eq!(
            super::spray_sizes(256, 256, 30.0, 64.0),
            ((30, 30), (64.0, 64.0))
        );
        assert_eq!(
            super::spray_sizes(200, 100, 30.0, 64.0),
            ((30, 15), (64.0, 32.0))
        );
    }

    #[test]
    fn shrink_averages_by_alpha() {
        // A red opaque pixel and a clear one become one half-clear red pixel.
        let img = macroquad::texture::Image {
            bytes: vec![255, 0, 0, 255, 0, 0, 255, 0],
            width: 2,
            height: 1,
        };
        let out = super::shrink(&img, 1, 1);
        assert_eq!(out.bytes, vec![255, 0, 0, 128]);
    }

    #[test]
    fn krp_tables_list_classes_and_weapons() {
        let (classes, weapons) = super::krp_loadouts();
        assert!(classes.len() >= 9);
        assert_eq!(classes[0].folder, "triggerman");
        assert!(weapons.iter().any(|w| w == "smg"));
    }
}

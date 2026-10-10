//! Maps: the pixel grid a map is stored as, and the tiles, barrels and
//! pickups a round is played on.
//!
//! A map is image data (`genData`: width, height, RGBA bytes). Client and
//! server both turn it into tiles with KRP's `setupMap` (ported here as
//! [`World::new`]): black is wall, `0 255 0` a healthpack floor,
//! `255 255 0` a hardpoint in Hardpoint and Zone War (a lootcrate floor
//! otherwise), red and blue spawn cells. The outermost ring is wall without
//! collision, and world coordinates start two tiles in. The client builds
//! its own tiles from the same pixels, so this must match it exactly.
//!
//! Where maps come from is [`super::maps`]'s concern.

use serde::Serialize;
use serde_json::{Value, json};

use super::data::Mode;

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A map as stored: row-major pixel colours.
#[derive(Debug, Clone)]
pub struct Map {
    pub width: usize,
    pub height: usize,
    rgb: Vec<[u8; 3]>,
}

impl Map {
    /// Parses a text map: `#` wall, `.` floor, `g` healthpack, `y` yellow,
    /// `r` red and `b` blue spawn cells. Blank lines and lines starting
    /// with `;` are ignored.
    ///
    /// # Errors
    /// Fails on unknown characters, ragged rows, or a map too small to play.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let rows: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty() && !l.starts_with(';'))
            .collect();
        let height = rows.len();
        let width = rows.first().map_or(0, |r| r.chars().count());
        let mut rgb = Vec::with_capacity(width * height);
        for (y, row) in rows.iter().enumerate() {
            if row.chars().count() != width {
                return Err(Error(format!("row {y} has a different width")));
            }
            for ch in row.chars() {
                rgb.push(match ch {
                    '#' => [0, 0, 0],
                    '.' => [255, 255, 255],
                    'g' => [0, 255, 0],
                    'y' => [255, 255, 0],
                    'r' => [255, 0, 0],
                    'b' => [0, 0, 255],
                    other => return Err(Error(format!("unknown map character {other:?}"))),
                });
            }
        }
        Self::from_rgb(width, height, rgb)
    }

    /// Builds a map from `genData`: `{width, height, data: [r, g, b, a, ...]}`,
    /// or with the bytes under `data.data`.
    ///
    /// # Errors
    /// Fails if the object is malformed or the map too small to play.
    pub fn from_gen_data(gen_data: &Value) -> Result<Self, Error> {
        let dim = |k: &str| {
            gen_data
                .get(k)
                .and_then(Value::as_u64)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| Error(format!("genData.{k} is missing")))
        };
        let (width, height) = (dim("width")?, dim("height")?);
        let data = gen_data
            .pointer("/data/data")
            .or_else(|| gen_data.get("data"))
            .and_then(Value::as_array)
            .ok_or_else(|| Error("genData.data is missing".into()))?;
        if data.len() != width * height * 4 {
            return Err(Error(format!(
                "genData has {} bytes for {width}x{height}",
                data.len()
            )));
        }
        let bytes: Vec<u8> = data
            .iter()
            .map(|v| v.as_u64().and_then(|b| u8::try_from(b).ok()))
            .collect::<Option<_>>()
            .ok_or_else(|| Error("genData bytes must be 0-255".into()))?;
        let rgb = bytes.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect();
        Self::from_rgb(width, height, rgb)
    }

    /// Builds a map from row-major pixel colours.
    ///
    /// # Errors
    /// Fails if the sizes disagree or the map is too small or too large.
    pub fn from_rgb(width: usize, height: usize, rgb: Vec<[u8; 3]>) -> Result<Self, Error> {
        if width < 5 || height < 5 {
            return Err(Error("map must be at least 5x5".into()));
        }
        if width > 200 || height > 200 {
            return Err(Error("map must be at most 200x200".into()));
        }
        if rgb.len() != width * height {
            return Err(Error("pixel count does not match the size".into()));
        }
        Ok(Self { width, height, rgb })
    }

    /// `genData` for the client, with the original colours.
    #[must_use]
    pub fn gen_data(&self) -> Value {
        let mut data = Vec::with_capacity(self.rgb.len() * 4);
        for [r, g, b] in &self.rgb {
            data.extend([json!(r), json!(g), json!(b), json!(255)]);
        }
        json!({"width": self.width, "height": self.height, "data": data})
    }

    fn pixel(&self, col: usize, row: usize) -> [u8; 3] {
        self.rgb[row * self.width + col]
    }
}

/// One tile, with KRP's meaning for each field.
// Flags mirror KRP's object fields one for one.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default)]
pub struct Tile {
    pub scale: f64,
    pub x: f64,
    pub y: f64,
    pub wall: bool,
    pub sprite_index: u8,
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
    pub has_collision: bool,
    pub hard_point: bool,
    /// `red`, `blue` or `e` (none).
    pub obj_team: &'static str,
    pub edge_tile: bool,
}

/// A barrel. `i` is 1 for a plain barrel, 2 for an explosive one.
#[derive(Debug, Clone, Serialize)]
pub struct Clutter {
    pub x: f64,
    pub y: f64,
    pub active: bool,
    pub indx: usize,
    pub i: u8,
    pub w: f64,
    pub h: f64,
    pub hc: bool,
    pub tp: f64,
    pub s: bool,
}

/// A healthpack or lootcrate.
#[derive(Debug, Clone, Serialize)]
pub struct Pickup {
    pub x: f64,
    pub y: f64,
    pub active: bool,
    pub scale: f64,
    #[serde(rename = "type")]
    pub kind: &'static str,
}

/// The playing field of one round.
#[derive(Debug, Clone)]
pub struct World {
    pub map: Map,
    pub scale: f64,
    /// Column-major, as the client builds them.
    pub tiles: Vec<Tile>,
    pub clutter: Vec<Clutter>,
    pub pickups: Vec<Pickup>,
    /// Indexes of tiles players spawn on.
    pub spawn_tiles: Vec<usize>,
    /// Indexes of hardpoint tiles that score (Hardpoint, Zone War).
    pub score_tiles: Vec<usize>,
}

/// The player-shaped part of collision.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub old_x: f64,
    pub old_y: f64,
    pub width: f64,
    pub height: f64,
    pub jump_y: f64,
}

impl World {
    /// Lays out `map` for `mode` (KRP `setupMap` plus `Game.newMap`).
    /// `random(lo, hi)` is an inclusive random integer, for barrels.
    pub fn new(
        map: &Map,
        scale: f64,
        mode: &Mode,
        random: &mut impl FnMut(i64, i64) -> i64,
    ) -> Self {
        let per_col = map.height;
        let start = -(scale * 2.0);
        let units = |n: usize| f64::from(u32::try_from(n).unwrap_or(u32::MAX));
        let mut tiles: Vec<Tile> = Vec::with_capacity(map.width * map.height);
        for col in 0..map.width {
            for row in 0..map.height {
                let i = tiles.len();
                let mut rgb = map.pixel(col, row);
                if col == 0 && row == 0 {
                    rgb = [0, 0, 0];
                }
                let mut t = Tile {
                    scale,
                    x: start + scale * units(col),
                    y: start + scale * units(row),
                    obj_team: "e",
                    ..Tile::default()
                };
                let left = i.checked_sub(per_col);
                // KRP reads `tiles[i - 1]`, which crosses columns at row 0.
                let prev = i.checked_sub(1);
                if rgb == [0, 0, 0] {
                    t.wall = true;
                    t.has_collision = true;
                    if let Some(l) = left {
                        if tiles[l].wall {
                            t.left = true;
                        }
                        tiles[l].right = true;
                    }
                    if let Some(u) = prev {
                        if tiles[u].wall {
                            t.top = true;
                        }
                        tiles[u].bottom = true;
                    }
                    if col == 0 || row == 0 || col + 1 >= map.width || row + 1 >= map.height {
                        t.left = true;
                        t.right = true;
                        t.top = true;
                        t.bottom = true;
                        t.edge_tile = true;
                    }
                } else {
                    if left.is_some_and(|l| tiles[l].wall) {
                        t.left = true;
                    }
                    if prev.is_some_and(|u| tiles[u].wall) {
                        t.top = true;
                    }
                    match rgb {
                        [0, 255, 0] => t.sprite_index = 2,
                        [255, 255, 0] => {
                            if mode.name == "Hardpoint" || mode.name == "Zone War" {
                                t.hard_point = true;
                                if mode.name == "Zone War" {
                                    t.obj_team = if col < map.width / 2 { "red" } else { "blue" };
                                }
                            } else {
                                t.sprite_index = 1;
                            }
                        }
                        [255, 0, 0] => t.obj_team = "red",
                        [0, 0, 255] if mode.teams => t.obj_team = "blue",
                        _ => {}
                    }
                }
                tiles.push(t);
            }
        }
        for t in &mut tiles {
            if t.edge_tile {
                t.has_collision = false;
            }
        }
        let mut spawn_tiles = Vec::new();
        let mut score_tiles = Vec::new();
        for (i, t) in tiles.iter().enumerate() {
            if !t.hard_point {
                if t.obj_team == "red" || (t.obj_team == "blue" && mode.teams) {
                    spawn_tiles.push(i);
                }
            } else if mode.code == "hp" || mode.code == "zmtch" {
                score_tiles.push(i);
            }
        }
        let mut world = Self {
            map: map.clone(),
            scale,
            tiles,
            clutter: Vec::new(),
            pickups: Vec::new(),
            spawn_tiles,
            score_tiles,
        };
        world.gen_clutter(random);
        world.gen_pickups(mode);
        world
    }

    /// KRP `genClutter`: about one in eleven plain floor tiles beside a
    /// wall gets a barrel against that wall, plain or explosive.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn gen_clutter(&mut self, random: &mut impl FnMut(i64, i64) -> i64) {
        let mut next = 0;
        for t in &self.tiles {
            if t.sprite_index != 0 || t.wall {
                continue;
            }
            if random(0, 10) > 0 {
                continue;
            }
            let sprite = if random(1, 2) == 2 { 2 } else { 1 };
            let (w, h) = (48.0, 84.0);
            let quarter = t.scale / 4.0;
            let rand_y = random(quarter as i64, (quarter * 3.0) as i64) as f64;
            let (x, y) = if t.left {
                (t.x, t.y + rand_y)
            } else if t.right {
                (t.x + t.scale - w, t.y + rand_y)
            } else {
                continue;
            };
            self.clutter.push(Clutter {
                x,
                y,
                active: true,
                indx: next,
                i: sprite,
                w,
                h,
                hc: true,
                tp: 1.0,
                s: true,
            });
            next += 1;
        }
    }

    /// KRP `genPickups`: healthpacks on green, lootcrates on yellow in
    /// Lootcrate (off until the loot timer turns them on).
    fn gen_pickups(&mut self, mode: &Mode) {
        let mid = self.scale / 2.0;
        for t in &self.tiles {
            let (kind, active) = match t.sprite_index {
                2 => ("healthpack", true),
                1 if mode.code == "lc" => ("lootcrate", false),
                _ => continue,
            };
            self.pickups.push(Pickup {
                x: t.x + mid,
                y: t.y + mid,
                active,
                scale: 64.0,
                kind,
            });
        }
    }

    /// The world size the client draws (`(width - 4) * scale`).
    #[must_use]
    pub fn size(&self) -> (f64, f64) {
        let units = |n: usize| f64::from(u32::try_from(n.saturating_sub(4)).unwrap_or(0));
        (
            units(self.map.width) * self.scale,
            units(self.map.height) * self.scale,
        )
    }

    /// `mapData` for `gameSetup`. The client rebuilds tiles itself, so
    /// none are sent.
    #[must_use]
    pub fn client_json(&self, mode: &Mode) -> Value {
        let (w, h) = self.size();
        json!({
            "gameMode": mode.client_json(),
            "genData": self.map.gen_data(),
            "tiles": [],
            "clutter": self.clutter,
            "pickups": self.pickups,
            "width": w,
            "height": h,
        })
    }

    /// KRP `wallCol`: pushes a moving body out of walls and barrels, and
    /// returns the name offset for a head under a wall.
    pub fn wall_col(&self, b: &mut Body) -> f64 {
        let walls = || self.tiles.iter().filter(|t| t.wall && t.has_collision);
        let barrels = || self.clutter.iter().filter(|c| c.active && c.hc);
        let touches_tile = |x: f64, y: f64, w: f64, t: &Tile| {
            x + w / 2.0 >= t.x && x - w / 2.0 <= t.x + t.scale && y >= t.y && y <= t.y + t.scale
        };
        let touches_clutter = |x: f64, y: f64, w: f64, c: &Clutter| {
            x + w / 2.0 >= c.x
                && x - w / 2.0 <= c.x + c.w
                && y >= c.y - (c.h / 2.0) * c.tp
                && y <= c.y + (c.h / 2.0) * c.tp
        };
        for t in walls() {
            if touches_tile(b.x, b.old_y, b.width, t) {
                if b.old_x + b.width / 2.0 <= t.x {
                    b.x = t.x - b.width / 2.0 - 2.0;
                } else if b.old_x - b.width / 2.0 >= t.x + t.scale {
                    b.x = t.x + t.scale + b.width / 2.0 + 2.0;
                }
            }
        }
        for c in barrels() {
            if touches_clutter(b.x, b.old_y, b.width, c) {
                if b.old_x + b.width / 2.0 <= c.x {
                    b.x = c.x - b.width / 2.0 - 1.0;
                } else if b.old_x - b.width / 2.0 >= c.x + c.w {
                    b.x = c.x + c.w + b.width / 2.0 + 1.0;
                }
            }
        }
        for t in walls() {
            if touches_tile(b.x, b.y, b.width, t) {
                if b.old_y <= t.y {
                    b.y = t.y - 2.0;
                } else if b.old_y >= t.y + t.scale {
                    b.y = t.y + t.scale + 2.0;
                }
            }
        }
        for c in barrels() {
            if touches_clutter(b.x, b.y, b.width, c) {
                if b.old_y >= c.y + (c.h / 2.0) * c.tp {
                    b.y = c.y + (c.h / 2.0) * c.tp + 1.0;
                } else if b.old_y <= c.y - (c.h / 2.0) * c.tp {
                    b.y = c.y - (c.h / 2.0) * c.tp - 1.0;
                }
            }
        }
        let head = b.y - b.jump_y - b.height * 0.85;
        let mut name_y = 0.0;
        for t in walls() {
            if !t.hard_point
                && b.x > t.x
                && b.x < t.x + t.scale
                && head > t.y - t.scale / 2.0
                && head <= t.y
            {
                name_y = (head - t.y + t.scale / 2.0).round();
            }
        }
        name_y
    }
}

/// KRP `dotInRect`.
#[must_use]
pub fn dot_in_rect(px: f64, py: f64, x: f64, y: f64, w: f64, h: f64) -> bool {
    x <= px && px <= x + w && y <= py && py <= y + h
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::game::data::committed;

    pub(crate) const SPAWNS: &str = "\
#########
#########
##r.g.b##
##.....##
##..y..##
##r...b##
#########
#########
#########
";

    /// Never places a barrel (`random(0, 10) > 0`), otherwise the low end.
    pub(crate) fn no_random(lo: i64, _hi: i64) -> i64 {
        if lo == 0 { 1 } else { lo }
    }

    pub(crate) fn mode(code: &str) -> Mode {
        let d = committed("krp");
        d.modes.into_iter().find(|m| m.code == code).unwrap()
    }

    #[test]
    fn layout_matches_the_client() {
        let m = Map::parse(SPAWNS).unwrap();
        let w = World::new(&m, 100.0, &mode("ffa"), &mut no_random);
        assert_eq!(w.size(), (500.0, 500.0));
        // Column-major: tile (col 2, row 3) is index 2 * 9 + 3.
        let t = &w.tiles[2 * 9 + 3];
        assert!((t.x - 0.0).abs() < 1e-9 && (t.y - 100.0).abs() < 1e-9);
        assert!(!t.wall && t.left, "floor next to the left wall");
        assert!(w.tiles[0].wall && w.tiles[0].edge_tile && !w.tiles[0].has_collision);
        assert!(w.tiles[9 + 1].has_collision);
        // Free for all spawns on red only; blue counts in team modes.
        assert_eq!(w.spawn_tiles.len(), 2);
        let tdm = World::new(&m, 100.0, &mode("tdm"), &mut no_random);
        assert_eq!(tdm.spawn_tiles.len(), 4);
        assert_eq!(w.pickups.len(), 1, "one healthpack");
        let gd = w.map.gen_data();
        assert_eq!(gd["data"].as_array().unwrap().len(), 9 * 9 * 4);
        let again = Map::from_gen_data(&gd).unwrap();
        assert_eq!(again.gen_data(), gd);
    }

    #[test]
    fn yellow_is_a_hardpoint_only_in_objective_modes() {
        let m = Map::parse(SPAWNS).unwrap();
        let hard = |w: &World| w.tiles.iter().filter(|t| t.hard_point).count();
        assert_eq!(
            hard(&World::new(&m, 100.0, &mode("ffa"), &mut no_random)),
            0
        );
        let hp = World::new(&m, 100.0, &mode("hp"), &mut no_random);
        assert_eq!((hard(&hp), hp.score_tiles.len()), (1, 1));
        let lc = World::new(&m, 100.0, &mode("lc"), &mut no_random);
        assert!(
            lc.pickups
                .iter()
                .any(|p| p.kind == "lootcrate" && !p.active)
        );
    }

    #[test]
    fn barrels_stand_against_walls() {
        let m = Map::parse(SPAWNS).unwrap();
        let mut always = |lo: i64, _hi: i64| lo;
        let w = World::new(&m, 100.0, &mode("ffa"), &mut always);
        assert!(!w.clutter.is_empty());
        for c in &w.clutter {
            let t = w
                .tiles
                .iter()
                .find(|t| !t.wall && dot_in_rect(c.x + 1.0, c.y - 1.0, t.x, t.y, t.scale, t.scale))
                .expect("barrel on a floor tile");
            assert!(t.left || t.right);
        }
    }

    #[test]
    fn walls_push_bodies_back() {
        let m = Map::parse(SPAWNS).unwrap();
        let w = World::new(&m, 100.0, &mode("ffa"), &mut no_random);
        let mut b = Body {
            x: -5.0,
            y: 50.0,
            old_x: 30.0,
            old_y: 50.0,
            width: 50.0,
            height: 94.0,
            jump_y: 0.0,
        };
        w.wall_col(&mut b);
        assert!((b.x - 27.0).abs() < 1e-9, "x = {}", b.x);
    }

    #[test]
    fn rejects_bad_maps() {
        assert!(Map::parse("##\n##\n").is_err());
        assert!(Map::parse(&SPAWNS.replace("r.g", "r.x")).is_err());
        let bad = json!({"width": 6, "height": 6, "data": [0, 0, 0]});
        assert!(Map::from_gen_data(&bad).is_err());
    }
}

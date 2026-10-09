//! Maps: the pixel grid the client turns into tiles, and the same tiles on
//! the server for collision and spawning.
//!
//! The client receives a map as image data (`genData`: width, height and
//! RGBA bytes) and builds its tiles from the colours: black is wall,
//! `0 255 0` and `255 255 0` are floor variants (the latter is a hardpoint
//! in Hardpoint and Zone War), anything else is plain floor. The outermost
//! ring is drawn as wall without collision, and world coordinates start
//! two tiles in. This module mirrors that layout exactly, because the
//! client predicts its own movement against it.
//!
//! The pixels are kept as they came, so the client gets the same colours
//! the map file has. Red and blue pixels look like floor to the client;
//! the server reads them as spawn cells (INFERRED, research #62).
//!
//! Where maps come from is [`super::maps`]'s concern.

use serde_json::{Value, json};

/// The client's rule for a pixel, by exact colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Wall,
    Floor,
    /// `0 255 0`: the client draws a different floor sprite.
    Green,
    /// `255 255 0`: hardpoint in Hardpoint/Zone War, a floor variant otherwise.
    Yellow,
}

impl Cell {
    #[must_use]
    pub fn of(rgb: [u8; 3]) -> Self {
        match rgb {
            [0, 0, 0] => Self::Wall,
            [0, 255, 0] => Self::Green,
            [255, 255, 0] => Self::Yellow,
            _ => Self::Floor,
        }
    }
}

/// Spawn cell colours (server-side only).
pub const RED: [u8; 3] = [255, 0, 0];
pub const BLUE: [u8; 3] = [0, 0, 255];

/// A tile as the server needs it (positions in world pixels).
#[derive(Debug, Clone, Copy)]
pub struct Tile {
    pub x: f64,
    pub y: f64,
    /// The pixel's colour.
    pub rgb: [u8; 3],
    pub wall: bool,
    pub has_collision: bool,
    pub hard_point: bool,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub width: usize,
    pub height: usize,
    /// Row-major pixel colours, as in `genData`.
    rgb: Vec<[u8; 3]>,
    pub scale: f64,
    /// Column-major, the order the client builds them in.
    pub tiles: Vec<Tile>,
}

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Map sizes are a few dozen tiles, so `usize` to `f64` is exact.
#[allow(clippy::cast_precision_loss)]
fn units(n: usize) -> f64 {
    n as f64
}

impl Map {
    /// Parses a text map: `#` wall, `.` floor, `g` green floor, `y`
    /// yellow, `r` red and `b` blue spawn cells. Blank lines and lines
    /// starting with `;` are ignored.
    ///
    /// # Errors
    /// Fails on unknown characters, ragged rows, or a map too small to play.
    pub fn parse(text: &str, scale: f64, hardpoints: bool) -> Result<Self, Error> {
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
                    'r' => RED,
                    'b' => BLUE,
                    other => return Err(Error(format!("unknown map character {other:?}"))),
                });
            }
        }
        Self::from_rgb(width, height, rgb, scale, hardpoints)
    }

    /// Builds a map from `genData` as the client receives it:
    /// `{width, height, data: {data: [r, g, b, a, ...]}}`.
    ///
    /// # Errors
    /// Fails if the object is malformed or the map too small to play.
    pub fn from_gen_data(gen_data: &Value, scale: f64, hardpoints: bool) -> Result<Self, Error> {
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
            .and_then(Value::as_array)
            .ok_or_else(|| Error("genData.data.data is missing".into()))?;
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
        Self::from_rgb(width, height, rgb, scale, hardpoints)
    }

    /// Builds a map from row-major pixel colours.
    ///
    /// # Errors
    /// Fails if the sizes disagree or the map is too small to play.
    pub fn from_rgb(
        width: usize,
        height: usize,
        rgb: Vec<[u8; 3]>,
        scale: f64,
        hardpoints: bool,
    ) -> Result<Self, Error> {
        if width < 6 || height < 6 {
            return Err(Error("map must be at least 6x6".into()));
        }
        if rgb.len() != width * height {
            return Err(Error("pixel count does not match the size".into()));
        }
        let mut map = Self {
            width,
            height,
            rgb,
            scale,
            tiles: Vec::new(),
        };
        map.build_tiles(hardpoints);
        Ok(map)
    }

    /// The same map laid out for a mode: the client turns yellow pixels
    /// into hardpoints only in modes with these names (RECOVERED:
    /// `setupMap` compares `gameMode.name`).
    #[must_use]
    pub fn for_mode(&self, mode_name: &str) -> Self {
        let mut m = self.clone();
        m.build_tiles(mode_name == "Hardpoint" || mode_name == "Zone War");
        m
    }

    fn pixel(&self, col: usize, row: usize) -> [u8; 3] {
        // The client forces the top-left pixel to wall.
        if col == 0 && row == 0 {
            [0, 0, 0]
        } else {
            self.rgb[row * self.width + col]
        }
    }

    fn build_tiles(&mut self, hardpoints: bool) {
        let origin = -2.0 * self.scale;
        let mut tiles = Vec::with_capacity(self.width * self.height);
        for col in 0..self.width {
            for row in 0..self.height {
                let rgb = self.pixel(col, row);
                let cell = Cell::of(rgb);
                let wall = cell == Cell::Wall;
                let edge = col == 0 || row == 0 || col >= self.width - 1 || row >= self.height - 1;
                tiles.push(Tile {
                    x: origin + self.scale * units(col),
                    y: origin + self.scale * units(row),
                    rgb,
                    wall,
                    has_collision: wall && !edge,
                    hard_point: !wall && hardpoints && cell == Cell::Yellow,
                });
            }
        }
        self.tiles = tiles;
    }

    /// World size the client computes: the grid minus the 2-tile border.
    #[must_use]
    pub fn world_size(&self) -> (f64, f64) {
        (
            (units(self.width) - 4.0) * self.scale,
            (units(self.height) - 4.0) * self.scale,
        )
    }

    /// `mapData.genData` as the client reads it.
    #[must_use]
    pub fn gen_data(&self) -> Value {
        let mut data = Vec::with_capacity(self.width * self.height * 4);
        for row in 0..self.height {
            for col in 0..self.width {
                let [r, g, b] = self.rgb[row * self.width + col];
                data.extend([r, g, b, 255]);
            }
        }
        json!({"width": self.width, "height": self.height, "data": {"data": data}})
    }

    /// Where a player of `team` may spawn: the map's spawn cells (red, or
    /// blue for the blue team; both in free-for-all), or any open floor if
    /// the map has none (ASSUMED).
    #[must_use]
    pub fn spawn_tiles(&self, team: Option<&str>) -> Vec<&Tile> {
        let open = |t: &&Tile| !t.wall && !t.hard_point;
        let marked: Vec<&Tile> = self
            .tiles
            .iter()
            .filter(open)
            .filter(|t| match team {
                Some("blue") => t.rgb == BLUE,
                Some(_) => t.rgb == RED,
                None => t.rgb == RED || t.rgb == BLUE,
            })
            .collect();
        if marked.is_empty() {
            self.tiles.iter().filter(open).collect()
        } else {
            marked
        }
    }
}

/// The parts of a moving player that wall collision needs.
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

impl Map {
    /// The client's `wallCol` for tiles: pushes `body` out of any wall it
    /// entered and returns the name offset the client computes (how far a
    /// wall above hides the name tag).
    pub fn collide(&self, body: &mut Body) -> f64 {
        let s = self.scale;
        let mut name_y_offset = 0.0;
        for t in self.tiles.iter().filter(|t| t.wall && t.has_collision) {
            if body.x + body.width / 2.0 >= t.x
                && body.x - body.width / 2.0 <= t.x + s
                && body.y >= t.y
                && body.y <= t.y + s
            {
                if body.old_x <= t.x {
                    body.x = t.x - body.width / 2.0 - 2.0;
                } else if body.old_x - body.width / 2.0 >= t.x + s {
                    body.x = t.x + s + body.width / 2.0 + 2.0;
                }
                if body.old_y <= t.y {
                    body.y = t.y - 2.0;
                } else if body.old_y >= t.y + s {
                    body.y = t.y + s + 2.0;
                }
            }
            let head = body.y - body.jump_y - 0.85 * body.height;
            if !t.hard_point
                && body.x > t.x
                && body.x < t.x + s
                && head > t.y - s / 2.0
                && head <= t.y
            {
                name_y_offset = (head - (t.y - s / 2.0)).round();
            }
        }
        name_y_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "\
######
######
##..##
##..##
######
######
";

    #[test]
    fn layout_matches_the_client() {
        let m = Map::parse(SMALL, 100.0, false).unwrap();
        assert_eq!(m.world_size(), (200.0, 200.0));
        // Column-major: tile (col 2, row 3) is index 2*6 + 3.
        let t = m.tiles[2 * 6 + 3];
        assert!((t.x - 0.0).abs() < 1e-9 && (t.y - 100.0).abs() < 1e-9);
        assert!(!t.wall);
        // Border tiles are walls without collision; inner walls collide.
        assert!(m.tiles[0].wall && !m.tiles[0].has_collision);
        assert!(m.tiles[6 + 1].wall && m.tiles[6 + 1].has_collision);
        let gd = m.gen_data();
        assert_eq!(gd["data"]["data"].as_array().unwrap().len(), 6 * 6 * 4);
        assert_eq!(m.spawn_tiles(None).len(), 4);
    }

    #[test]
    fn walls_push_players_back() {
        let m = Map::parse(SMALL, 100.0, false).unwrap();
        // Walk left from inside the room into the wall at x in [-100, 0].
        let mut b = Body {
            x: -5.0,
            y: 50.0,
            old_x: 30.0,
            old_y: 50.0,
            width: 50.0,
            height: 94.0,
            jump_y: 0.0,
        };
        m.collide(&mut b);
        assert!((b.x - 27.0).abs() < 1e-9, "x = {}", b.x);
    }

    #[test]
    fn rejects_bad_maps() {
        assert!(Map::parse("##\n##\n", 100.0, false).is_err());
        assert!(Map::parse(&SMALL.replace("..##\n##..", "..##\n##.x"), 100.0, false).is_err());
    }

    const SPAWNS: &str = "\
#######
#######
##r.b##
##.y.##
##rgb##
#######
#######
";

    #[test]
    fn gen_data_round_trips_colours() {
        let m = Map::parse(SPAWNS, 100.0, false).unwrap();
        let again = Map::from_gen_data(&m.gen_data(), 100.0, false).unwrap();
        assert_eq!(again.gen_data(), m.gen_data());
        let odd = Map::from_rgb(6, 6, vec![[251, 251, 251]; 36], 100.0, false).unwrap();
        let px = odd.gen_data()["data"]["data"][4 * 7].clone();
        assert_eq!(px, json!(251), "unknown colours are passed through");
        let bad = json!({"width": 6, "height": 6, "data": {"data": [0, 0, 0]}});
        assert!(Map::from_gen_data(&bad, 100.0, false).is_err());
    }

    #[test]
    fn spawns_use_team_cells() {
        let m = Map::parse(SPAWNS, 100.0, false).unwrap();
        assert!(m.spawn_tiles(Some("red")).iter().all(|t| t.rgb == RED));
        assert_eq!(m.spawn_tiles(Some("red")).len(), 2);
        assert!(m.spawn_tiles(Some("blue")).iter().all(|t| t.rgb == BLUE));
        assert_eq!(m.spawn_tiles(None).len(), 4);
        // Without spawn cells, any open floor will do.
        let plain = Map::parse(SMALL, 100.0, false).unwrap();
        assert_eq!(plain.spawn_tiles(Some("blue")).len(), 4);
    }

    #[test]
    fn yellow_is_a_hardpoint_only_in_hardpoint_modes() {
        let m = Map::parse(SPAWNS, 100.0, false).unwrap();
        let hard = |m: &Map| m.tiles.iter().filter(|t| t.hard_point).count();
        assert_eq!(hard(&m.for_mode("Free For All")), 0);
        assert_eq!(hard(&m.for_mode("Hardpoint")), 1);
        assert_eq!(hard(&m.for_mode("Zone War")), 1);
    }
}

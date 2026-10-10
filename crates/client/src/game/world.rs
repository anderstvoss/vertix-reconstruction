//! The map as KRP's client draws it: `setupMap`'s tile flags (including the
//! corner flags only drawing uses), hardpoint flags, and the cached wall
//! and floor canvases (`getCachedWall`, `getCachedFloor`).
//!
//! Collision uses the shared crate's [`vertix_sim::map::World`], built from
//! the same pixels; this module only adds what drawing needs.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::f32::consts::PI;
use std::rc::Rc;

use vertix_sim::map::Map;

use crate::assets::Sprites;
use crate::game::model::{Flag, GameMode};
use crate::game::rand::random_int;
use crate::gfx::{Canvas, Image, Materials, Painter};

/// One tile with every field KRP's client sets.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default)]
pub struct RenderTile {
    pub scale: f64,
    pub x: f64,
    pub y: f64,
    pub wall: bool,
    pub sprite_index: usize,
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
    pub top_left: bool,
    pub top_right: bool,
    pub bottom_left: bool,
    pub bottom_right: bool,
    pub has_collision: bool,
    pub hard_point: bool,
    pub obj_team: String,
    pub edge_tile: bool,
}

const FLAG_WIDTH: f64 = 70.0;
const FLAG_HEIGHT: f64 = 152.0;
const FLAG_OFFSET: f64 = 40.0;
const FLAG_EDGE_INSET: f64 = 30.0;

/// KRP `setupMap`: tiles column by column, and the hardpoint flags.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn setup_map(
    map: &Map,
    rgb: &[[u8; 3]],
    scale: f64,
    mode: &GameMode,
) -> (Vec<RenderTile>, Vec<Flag>) {
    let per_col = map.height;
    let start = -(scale * 2.0);
    let mut tiles: Vec<RenderTile> = Vec::with_capacity(map.width * map.height);
    let get = |tiles: &Vec<RenderTile>, i: isize| -> Option<usize> {
        (i >= 0 && (i as usize) < tiles.len()).then_some(i as usize)
    };
    for col in 0..map.width {
        for row in 0..map.height {
            let i = tiles.len() as isize;
            let pc = per_col as isize;
            let mut key = rgb[row * map.width + col];
            if col == 0 && row == 0 {
                key = [0, 0, 0];
            }
            let mut t = RenderTile {
                scale,
                x: start + scale * col as f64,
                y: start + scale * row as f64,
                obj_team: String::from("e"),
                ..RenderTile::default()
            };
            if key == [0, 0, 0] {
                t.wall = true;
                t.has_collision = true;
                if let Some(l) = get(&tiles, i - pc) {
                    if tiles[l].wall {
                        t.left = true;
                    }
                    tiles[l].right = true;
                }
                if let Some(d) = get(&tiles, i - pc - 1) {
                    if tiles[d].wall {
                        tiles[d].sprite_index = 0;
                        t.top_left = true;
                        tiles[d].bottom_right = true;
                    }
                }
                if let Some(d) = get(&tiles, i - pc + 1) {
                    tiles[d].top_right = true;
                    if tiles[d].wall {
                        t.bottom_left = true;
                    }
                }
                if let Some(u) = get(&tiles, i - 1) {
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
                if get(&tiles, i - pc).is_some_and(|l| tiles[l].wall) {
                    t.left = true;
                }
                if get(&tiles, i - 1).is_some_and(|u| tiles[u].wall) {
                    t.top = true;
                }
                if get(&tiles, i - pc - 1).is_some_and(|d| tiles[d].wall) {
                    t.top_left = true;
                }
                match key {
                    [0, 255, 0] => t.sprite_index = 2,
                    [255, 255, 0] => {
                        if mode.name == "Hardpoint" || mode.name == "Zone War" {
                            t.hard_point = true;
                            if mode.name == "Zone War" {
                                t.obj_team =
                                    String::from(if col < map.width / 2 { "red" } else { "blue" });
                            }
                        } else {
                            t.sprite_index = 1;
                        }
                    }
                    [255, 0, 0] => t.obj_team = String::from("red"),
                    [0, 0, 255] if mode.teams => t.obj_team = String::from("blue"),
                    _ => {}
                }
            }
            tiles.push(t);
        }
    }
    let mut flags = Vec::new();
    let opposite = scale - FLAG_EDGE_INSET - FLAG_OFFSET;
    let can_place = |tiles: &Vec<RenderTile>, i: isize, ignore_walls: bool| -> bool {
        get(tiles, i).is_some_and(|j| {
            let t = &tiles[j];
            if ignore_walls {
                !t.wall && !t.hard_point
            } else {
                !t.hard_point
            }
        })
    };
    for idx in 0..tiles.len() {
        if tiles[idx].edge_tile {
            tiles[idx].has_collision = false;
            continue;
        }
        if tiles[idx].wall || !tiles[idx].hard_point {
            continue;
        }
        let i = idx as isize;
        let pc = per_col as isize;
        let mut push = |tile: &RenderTile, xo: f64, yo: f64| {
            flags.push(Flag {
                team: tile.obj_team.clone(),
                x: tile.x + xo,
                y: tile.y + yo,
                w: FLAG_WIDTH,
                h: FLAG_HEIGHT,
                ai: random_int(0, 2) as usize,
                ac: 0,
            });
        };
        let t = tiles[idx].clone();
        if can_place(&tiles, i - pc, true) && can_place(&tiles, i - 1, false) {
            push(&t, FLAG_OFFSET, FLAG_OFFSET);
        }
        if can_place(&tiles, i + pc, true) && can_place(&tiles, i - 1, false) {
            push(&t, opposite, FLAG_OFFSET);
        }
        if can_place(&tiles, i + pc, true) && can_place(&tiles, i + 1, false) {
            push(&t, opposite, opposite);
        }
        if can_place(&tiles, i - pc, true) && can_place(&tiles, i + 1, false) {
            push(&t, FLAG_OFFSET, opposite);
        }
    }
    (tiles, flags)
}

/// The pixel colours of a `genData` object, row-major.
#[must_use]
pub fn gen_data_rgb(gen_data: &serde_json::Value) -> Option<Vec<[u8; 3]>> {
    let data = gen_data
        .pointer("/data/data")
        .or_else(|| gen_data.get("data"))?
        .as_array()?;
    let bytes: Vec<u8> = data
        .iter()
        .map(|v| v.as_u64().and_then(|b| u8::try_from(b).ok()))
        .collect::<Option<_>>()?;
    Some(bytes.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect())
}

const TILES_PER_FLOOR_TILE: f32 = 8.0;

/// The cached wall and floor canvases, by KRP's cache keys.
pub struct TileCache {
    mats: Rc<Materials>,
    walls: HashMap<String, Canvas>,
    floors: HashMap<String, Canvas>,
}

impl TileCache {
    #[must_use]
    pub fn new(mats: Rc<Materials>) -> Self {
        Self {
            mats,
            walls: HashMap::new(),
            floors: HashMap::new(),
        }
    }

    /// KRP `getCachedWall`. Bakes on first use, then `restore` puts the
    /// caller's target back.
    pub fn wall(
        &mut self,
        t: &RenderTile,
        s: &Sprites,
        restore: &mut dyn FnMut(),
    ) -> Option<Image> {
        let b = |v: bool| if v { "1" } else { "0" };
        let key = format!(
            "{}{}{}{}{}{}{}{}{}{}",
            b(t.left),
            b(t.right),
            b(t.top),
            b(t.bottom),
            b(t.top_left),
            b(t.top_right),
            b(t.bottom_left),
            b(t.bottom_right),
            t.edge_tile,
            t.has_collision
        );
        if let Some(c) = self.walls.get(&key) {
            return Some(c.image());
        }
        let wall = s.wall.as_ref()?;
        let filler = s.dark_filler.as_ref();
        let sc = t.scale as f32;
        let canvas = Canvas::new(sc as u32, sc as u32);
        let mut p = Painter::new(Rc::clone(&self.mats));
        canvas.begin(&mut p);
        p.draw_image(wall, 0.0, 0.0, sc, sc);
        if let Some(f) = filler {
            let mut d = |x: f32, y: f32, w: f32, h: f32| {
                p.draw_image(f, x.floor(), y.floor(), w.floor(), h.floor());
            };
            d(12.0, 12.0, sc - 24.0, sc - 24.0);
            if t.left {
                d(0.0, 12.0, 12.0, sc - 24.0);
            }
            if t.right {
                d(sc - 12.0, 12.0, 12.0, sc - 24.0);
            }
            if t.top {
                d(12.0, 0.0, sc - 24.0, 12.0);
            }
            if t.bottom {
                d(12.0, sc - 12.0, sc - 24.0, 12.0);
            }
            if !t.has_collision || (t.top_left && t.top && t.left) {
                d(0.0, 0.0, 12.0, 12.0);
            }
            if !t.has_collision || (t.top_right && t.top && t.right) {
                d(sc - 12.0, 0.0, 12.0, 12.0);
            }
            if !t.has_collision || (t.bottom_left && t.bottom && t.left) {
                d(0.0, sc - 12.0, 12.0, 12.0);
            }
            if !t.has_collision || (t.bottom_right && t.bottom && t.right) {
                d(sc - 12.0, sc - 12.0, 12.0, 12.0);
            }
        }
        macroquad::prelude::gl_use_default_material();
        restore();
        let img = canvas.image();
        self.walls.insert(key, canvas);
        Some(img)
    }

    /// KRP `getCachedFloor`.
    pub fn floor(
        &mut self,
        t: &RenderTile,
        s: &Sprites,
        restore: &mut dyn FnMut(),
    ) -> Option<(Image, f32, f32)> {
        let b = |v: bool| if v { "1" } else { "0" };
        let key = format!(
            "{}{}{}{}{}{}{}",
            t.sprite_index,
            b(t.left),
            b(t.right),
            b(t.top),
            b(t.bottom),
            b(t.top_left),
            b(t.top_right)
        );
        if let Some(c) = self.floors.get(&key) {
            return Some((c.image(), c.width, c.height));
        }
        let sidewalk = s.sidewalk.as_ref()?;
        let sc = t.scale as f32;
        // Canvas sizes truncate to whole pixels.
        let h = (sc * if t.bottom { 0.51 } else { 1.0 }).floor();
        let canvas = Canvas::new(sc as u32, h as u32);
        let mut p = Painter::new(Rc::clone(&self.mats));
        canvas.begin(&mut p);
        if let Some(Some(floor)) = s.floors.get(t.sprite_index) {
            p.draw_image(floor, 0.0, 0.0, sc, sc);
        }
        let ambient = s.ambient.first().and_then(Option::as_ref);
        let unit = sc / TILES_PER_FLOOR_TILE;
        let mut walk = |count: f32, rot: Option<f32>, mut x: f32, mut y: f32, xi: f32, yi: f32| {
            for _ in 0..count as usize {
                p.draw_image(sidewalk, x, y, unit, unit);
                if let (Some(r), Some(a)) = (rot, ambient) {
                    p.save();
                    p.translate(x + unit / 2.0, y + unit / 2.0);
                    p.rotate(r);
                    p.draw_image(a, -(unit / 2.0), -(unit / 2.0), unit, unit);
                    p.restore();
                }
                x += xi;
                y += yi;
            }
        };
        if t.top_left {
            walk(1.0, Some(0.0), 0.0, 0.0, 0.0, 0.0);
        }
        if t.top_right {
            walk(1.0, Some(PI), sc - unit, 0.0, 0.0, 0.0);
        }
        if t.left {
            if t.top {
                walk(2.0, None, 0.0, 0.0, 0.0, unit);
                walk(
                    TILES_PER_FLOOR_TILE - 2.0,
                    Some(0.0),
                    0.0,
                    unit * 2.0,
                    0.0,
                    unit,
                );
            } else {
                walk(TILES_PER_FLOOR_TILE, Some(0.0), 0.0, 0.0, 0.0, unit);
            }
        }
        if t.right {
            if t.top {
                walk(2.0, None, sc - unit, 2.0, 0.0, unit);
                walk(
                    TILES_PER_FLOOR_TILE - 2.0,
                    Some(PI),
                    sc - unit,
                    unit * 2.0,
                    0.0,
                    unit,
                );
            } else {
                walk(TILES_PER_FLOOR_TILE, Some(PI), sc - unit, 0.0, 0.0, unit);
            }
        }
        if t.top {
            walk(TILES_PER_FLOOR_TILE, Some(PI / 2.0), 0.0, 0.0, unit, 0.0);
        }
        if t.bottom {
            walk(TILES_PER_FLOOR_TILE, Some(0.0), 0.0, sc - unit, unit, 0.0);
        }
        macroquad::prelude::gl_use_default_material();
        restore();
        let img = canvas.image();
        let (w, hh) = (canvas.width, canvas.height);
        self.floors.insert(key, canvas);
        Some((img, w, hh))
    }
}

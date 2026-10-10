//! Bullets: KRP's `Projectile` (`core/src/logic/projectile.ts`) without the
//! drawing. The server simulates with it; the client also reads [`Hit`]s
//! for its effects.
//!
//! The server keeps a pool of bullets per room. A shot activates the next
//! one; each tick moves it in `update_accuracy` sub-steps, checks barrels,
//! walls and players along the segment it covered, and records what it hit
//! for the room to apply.

use super::data::WeaponSpec;
use super::map::{Clutter, Tile, dot_in_rect};

const EXPLOSIVE_CLUTTER_INDEX: u8 = 2;
const BOUNCE_SPEED_RETENTION: f64 = 0.65;
const BULLET_IMMUNITY_MS: f64 = 250.0;
const LINE_INTERSECTION_EPSILON: f64 = 1e-7;
const COLLISION_ADJUST_ATTEMPTS: u32 = 100;
const COLLISION_ADJUST_STEP: f64 = 2.0;

/// What a bullet needs to know about a player to hit them.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub index: u32,
    pub team: &'a str,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub jump_y: f64,
    pub on_screen: bool,
    pub dead: bool,
    pub spawn_protected: bool,
}

/// The shooter, as the bullet remembers them.
#[derive(Debug, Clone, Default)]
pub struct Owner {
    pub index: u32,
    pub team: String,
    pub height: f64,
}

// Flags mirror KRP's object fields one for one.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default)]
pub struct Projectile {
    pub server_index: usize,
    pub active: bool,
    pub x: f64,
    pub y: f64,
    pub start_x: f64,
    pub start_y: f64,
    pub dir: f64,
    pub c_end_x: f64,
    pub c_end_y: f64,
    pub speed: f64,
    pub width: f64,
    pub height: f64,
    pub y_offset: f64,
    pub jump_y: f64,
    pub trail_max_length: f64,
    pub update_accuracy: u32,
    pub sprite_index: u32,
    pub owner: Owner,
    pub dmg: f64,
    pub pierce_count: u32,
    pub blast_radius: Option<f64>,
    pub bounce: bool,
    pub explode_on_death: bool,
    pub self_damage: bool,
    pub collides_with_explosive_clutter: bool,
    pub start_time: f64,
    pub max_life_time: Option<f64>,
    pub skip_move: bool,
    pub hit_clutter: Vec<usize>,
    pub hit_players: Vec<u32>,
    pub player_immunity: Vec<(u32, f64)>,
    /// The direction the shot was fired in (explosions push this way).
    pub fire_dir: f64,
    /// What it hit in the last update.
    pub hits: Vec<Hit>,
}

/// Something a bullet hit during the last update, for the client's
/// effects (KRP's client spawns sparks and blood where these happen).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// A wall or barrel, at the bullet's collision end. `flip_y` is
    /// KRP's `hitSomething` argument.
    Surface { x: f64, y: f64, flip_y: bool },
    /// A player took the bullet.
    Player { index: u32 },
}

/// A shot as the room announces it (`"2"`): origin, direction, bullet slot.
#[derive(Debug, Clone, Copy)]
pub struct Shot {
    pub x: f64,
    pub y: f64,
    pub dir: f64,
    pub server_index: usize,
}

impl Projectile {
    /// KRP `shootNextBullet`: arms this bullet for `weapon`.
    /// `rand_scale` is a draw from the weapon's `bRandScale` range.
    #[allow(clippy::too_many_arguments)]
    pub fn shoot(
        &mut self,
        shot: Shot,
        weapon: &WeaponSpec,
        spread_now: f64,
        owner: Owner,
        jump_y: f64,
        target_d: f64,
        now: f64,
        rand_scale: Option<f64>,
    ) {
        self.server_index = shot.server_index;
        self.x = shot.x - 1.0;
        self.start_x = shot.x;
        self.y = shot.y;
        self.start_y = shot.y;
        self.dir = shot.dir;
        self.fire_dir = shot.dir;
        self.speed = weapon.b_speed;
        self.update_accuracy = weapon.c_acc.max(1);
        self.width = weapon.b_width;
        self.height = weapon.b_height;
        if let Some(r) = rand_scale {
            self.width *= r;
            self.height *= r;
            self.speed *= 1.0 + spread_now;
        }
        self.trail_max_length = (self.height * 5.0).round();
        self.sprite_index = weapon.b_sprite;
        self.y_offset = weapon.y_offset;
        self.jump_y = jump_y;
        self.owner = owner;
        self.dmg = weapon.dmg;
        self.bounce = weapon.bounce;
        self.start_time = now;
        self.max_life_time = weapon.max_life;
        if weapon.dist_based && self.speed > 0.0 {
            self.max_life_time = Some(target_d / self.speed);
        }
        self.explode_on_death = weapon.explode_on_death;
        self.pierce_count = weapon.pierce;
        self.blast_radius = weapon.blast_radius;
        self.self_damage = weapon.self_damage;
        self.collides_with_explosive_clutter = false;
        self.skip_move = true;
        self.hit_clutter.clear();
        self.hit_players.clear();
        self.player_immunity.clear();
        self.active = true;
    }

    /// KRP `Projectile.update` for `delta` milliseconds at time `now`.
    #[allow(clippy::too_many_lines)]
    pub fn update(
        &mut self,
        delta: f64,
        now: f64,
        clutter: &[Clutter],
        tiles: &[Tile],
        players: &[Target<'_>],
    ) {
        if !self.active {
            self.skip_move = false;
            return;
        }
        let mut lifetime = now - self.start_time;
        if self.skip_move {
            lifetime = 0.0;
            self.start_time = now;
        }
        self.hit_clutter.clear();
        self.hit_players.clear();
        self.hits.clear();
        let steps = f64::from(self.update_accuracy);
        for _ in 0..self.update_accuracy {
            let vel = self.speed * delta;
            for (_, left) in &mut self.player_immunity {
                *left -= delta / 3.0;
            }
            self.player_immunity.retain(|&(_, left)| left >= 0.0);
            if !self.active {
                continue;
            }
            let change_x = (vel * self.dir.cos()) / steps;
            let change_y = (vel * self.dir.sin()) / steps;
            if !self.skip_move && self.speed > 0.0 {
                self.x += change_x;
                self.y += change_y;
                if (self.x - self.start_x).hypot(self.y - self.start_y) >= self.trail_max_length {
                    self.start_x += change_x;
                    self.start_y += change_y;
                }
            }
            self.c_end_x = self.x + ((vel + self.height) * self.dir.cos()) / steps;
            self.c_end_y = self.y + ((vel + self.height) * self.dir.sin()) / steps;

            for (i, c) in clutter.iter().enumerate() {
                if self.active
                    && c.active
                    && c.hc
                    && self.can_see(c.x, c.y, c.h)
                    && c.h * c.tp >= self.y_offset
                    && self.line_in_rect(c.x, c.y - c.h, c.w, c.h * 0.7, true)
                {
                    if self.bounce {
                        self.bounce_dir(
                            self.c_end_y <= c.y - c.h || self.c_end_y >= c.y - c.h * 0.3,
                        );
                    } else {
                        self.clutter_hit(c, i);
                    }
                }
            }
            if self.active {
                for t in tiles {
                    if !self.active {
                        break;
                    }
                    if !(t.wall && t.has_collision && self.can_see(t.x, t.y, t.scale)) {
                        continue;
                    }
                    let hit = if t.bottom {
                        self.line_in_rect(t.x, t.y, t.scale, t.scale, true)
                    } else {
                        let h = t.scale - self.owner.height - self.jump_y;
                        self.line_in_rect(t.x, t.y, t.scale, h, true)
                    };
                    if hit {
                        self.active = false;
                        // KRP's `!(a <= b)`, which is also true for NaN.
                        let flip_y = !matches!(
                            self.c_end_x.partial_cmp(&t.x),
                            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                        ) && !matches!(
                            self.c_end_x.partial_cmp(&(t.x + t.scale)),
                            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                        );
                        if self.bounce {
                            self.bounce_dir(flip_y);
                        } else {
                            self.hits.push(Hit::Surface {
                                x: self.c_end_x,
                                y: self.c_end_y,
                                flip_y,
                            });
                        }
                    }
                }
            }
            if self.active {
                for p in players {
                    if p.index == self.owner.index
                        || self.hit_players.contains(&p.index)
                        || self.player_immunity.iter().any(|&(i, _)| i == p.index)
                        || p.team == self.owner.team
                        || !p.on_screen
                        || p.dead
                    {
                        continue;
                    }
                    let inside = self.line_in_rect(
                        p.x - p.width / 2.0,
                        p.y - p.height - p.jump_y,
                        p.width,
                        p.height,
                        self.pierce_count <= 1,
                    );
                    if inside && !p.spawn_protected {
                        if self.explode_on_death || self.collides_with_explosive_clutter {
                            self.active = false;
                        } else if self.dmg > 0.0 {
                            self.hits.push(Hit::Player { index: p.index });
                            self.hit_players.push(p.index);
                            self.player_immunity.push((p.index, BULLET_IMMUNITY_MS));
                            if self.pierce_count > 0 {
                                self.pierce_count -= 1;
                            }
                            if self.pierce_count == 0 {
                                self.active = false;
                            }
                        }
                    }
                    if !self.active {
                        break;
                    }
                }
            }
            if self.max_life_time.is_some_and(|m| lifetime >= m) {
                self.active = false;
            }
            if !self.active && self.explode_on_death && !self.collides_with_explosive_clutter {
                for (i, c) in clutter.iter().enumerate() {
                    if c.active
                        && c.hc
                        && self.can_see(c.x, c.y, c.h)
                        && c.h * c.tp >= self.y_offset
                        && self.in_explosion_range(c)
                    {
                        self.clutter_hit(c, i);
                    }
                }
            }
        }
        self.skip_move = false;
    }

    fn can_see(&self, x: f64, y: f64, size: f64) -> bool {
        (self.c_end_x - x).abs() <= (size + self.height) * 2.0
            && (self.c_end_y - y).abs() <= (size + self.height) * 2.0
    }

    fn bounce_dir(&mut self, flip_y: bool) {
        self.dir = if flip_y {
            std::f64::consts::PI * 2.0 - self.dir
        } else {
            std::f64::consts::PI - self.dir
        };
        self.active = true;
        self.speed *= BOUNCE_SPEED_RETENTION;
        self.x = self.c_end_x;
        self.y = self.c_end_y;
    }

    /// KRP `lineInRect`, including its variable reuse.
    fn line_in_rect(&mut self, rx: f64, ry: f64, rw: f64, rh: f64, adjust: bool) -> bool {
        let mut line_start_x = self.x;
        let line_start_y = self.y;
        let mut min_x = line_start_x;
        let mut max_x = self.c_end_x;
        if min_x > max_x {
            min_x = self.c_end_x;
            max_x = line_start_x;
        }
        if max_x > rx + rw {
            max_x = rx + rw;
        }
        if min_x < rx {
            min_x = rx;
        }
        if min_x > max_x {
            return false;
        }
        let mut min_y = line_start_y;
        let mut max_y = self.c_end_y;
        let dx = self.c_end_x - line_start_x;
        if dx.abs() > LINE_INTERSECTION_EPSILON {
            let slope = (self.c_end_y - line_start_y) / dx;
            line_start_x = line_start_y - slope * line_start_x;
            min_y = slope * min_x + line_start_x;
            max_y = slope * max_x + line_start_x;
        }
        if min_y > max_y {
            std::mem::swap(&mut min_y, &mut max_y);
        }
        if max_y > ry + rh {
            max_y = ry + rh;
        }
        if min_y < ry {
            min_y = ry;
        }
        if min_y > max_y {
            return false;
        }
        if adjust {
            self.adjust_on_collision(rx, ry, rw, rh);
        }
        true
    }

    fn adjust_on_collision(&mut self, rx: f64, ry: f64, rw: f64, rh: f64) {
        let mut cx = self.c_end_x;
        let mut cy = self.c_end_y;
        let back = self.dir + std::f64::consts::PI;
        let sx = back.cos() * COLLISION_ADJUST_STEP;
        let sy = back.sin() * COLLISION_ADJUST_STEP;
        for _ in 0..COLLISION_ADJUST_ATTEMPTS {
            if dot_in_rect(cx, cy, rx, ry, rw, rh) {
                break;
            }
            cx += sx;
            cy += sy;
        }
        for _ in 0..COLLISION_ADJUST_ATTEMPTS {
            if dot_in_rect(cx, cy, rx, ry, rw, rh) {
                cx += sx;
                cy += sy;
            } else {
                break;
            }
        }
        self.c_end_x = cx;
        self.c_end_y = cy;
        self.x = cx;
        self.y = cy;
    }

    fn in_explosion_range(&self, c: &Clutter) -> bool {
        let d = ((self.x + c.w / 2.0) - (c.x + c.w / 2.0)).hypot(self.y - (c.y - c.h / 2.0));
        self.blast_radius.is_some_and(|r| d <= r)
    }

    fn clutter_hit(&mut self, c: &Clutter, i: usize) {
        self.active = false;
        if c.i == EXPLOSIVE_CLUTTER_INDEX {
            self.collides_with_explosive_clutter = true;
            self.hit_clutter.push(i);
        }
        self.hits.push(Hit::Surface {
            x: self.c_end_x,
            y: self.c_end_y,
            flip_y: self.c_end_x > c.x && self.c_end_x < c.x + c.w && self.c_end_y > c.y - c.h,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::committed;
    use crate::map::tests::{SPAWNS, mode, no_random};
    use crate::map::{Map, World};

    fn target(index: u32, x: f64, y: f64) -> Target<'static> {
        Target {
            index,
            team: "b",
            x,
            y,
            width: 50.0,
            height: 94.0,
            jump_y: 0.0,
            on_screen: true,
            dead: false,
            spawn_protected: false,
        }
    }

    #[allow(clippy::many_single_char_names)]
    fn fire(weapon: &str, x: f64, y: f64, dir: f64) -> Projectile {
        let d = committed("krp");
        let w = &d
            .weapons
            .iter()
            .find(|w| w.spec.name == weapon)
            .unwrap()
            .spec;
        let mut p = Projectile::default();
        let owner = Owner {
            index: 0,
            team: "a".into(),
            height: 94.0,
        };
        let shot = Shot {
            x,
            y,
            dir,
            server_index: 3,
        };
        p.shoot(shot, w, 0.0, owner, 0.0, 400.0, 0.0, Some(1.0));
        p
    }

    #[test]
    fn a_bullet_hits_the_player_in_its_path() {
        let world = World::new(
            &Map::parse(SPAWNS).unwrap(),
            100.0,
            &mode("ffa"),
            &mut no_random,
        );
        // Fire right along y = 250 at a player standing 150 px away.
        let mut b = fire("smg", 50.0, 250.0, 0.0);
        let players = [target(1, 200.0, 290.0)];
        let mut hit = false;
        for step in 0..60 {
            b.update(
                16.0,
                f64::from(step) * 16.0,
                &world.clutter,
                &world.tiles,
                &players,
            );
            if !b.hit_players.is_empty() {
                hit = true;
                break;
            }
        }
        assert!(hit);
        assert!(!b.active, "smg does not pierce");
    }

    #[test]
    fn walls_stop_bullets() {
        let world = World::new(
            &Map::parse(SPAWNS).unwrap(),
            100.0,
            &mode("ffa"),
            &mut no_random,
        );
        // Fire left into the wall at x < 0.
        let mut b = fire("smg", 50.0, 150.0, std::f64::consts::PI);
        for step in 0..60 {
            b.update(
                16.0,
                f64::from(step) * 16.0,
                &world.clutter,
                &world.tiles,
                &[],
            );
            if !b.active {
                break;
            }
        }
        assert!(!b.active);
        assert!(b.x > -100.0, "stopped at the wall, x = {}", b.x);
    }

    #[test]
    fn teammates_and_protected_players_are_not_hit() {
        let world = World::new(
            &Map::parse(SPAWNS).unwrap(),
            100.0,
            &mode("ffa"),
            &mut no_random,
        );
        let mut b = fire("smg", 50.0, 250.0, 0.0);
        let mut mate = target(1, 200.0, 290.0);
        mate.team = "a";
        let mut safe = target(2, 200.0, 290.0);
        safe.spawn_protected = true;
        for step in 0..30 {
            b.update(
                16.0,
                f64::from(step) * 16.0,
                &world.clutter,
                &world.tiles,
                &[mate, safe],
            );
            assert!(b.hit_players.is_empty());
        }
    }
}

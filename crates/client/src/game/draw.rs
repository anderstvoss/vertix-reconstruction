//! KRP's `doGame` and everything it draws, in its order.

#![forbid(unsafe_code)]

use std::f64::consts::PI;

use macroquad::prelude::*;

use super::{Game, Gfx, MapState, TEAM_BLUE, TEAM_RED, snap_angle};
use crate::game::model::Player;
use crate::game::rand::random_int;
use crate::gfx::{Blend, Canvas, Image, Painter, hex};
use vertix_sim::projectile::Hit;

const TEXT_SIZE_MULT: f64 = 0.55;
const SHADOW_INTENSITY: f32 = 0.16;
const PLAYER_CANVAS_W: f32 = 300.0;
const PLAYER_CANVAS_H: f32 = 500.0;
pub const MINIMAP_SIZE: f32 = 200.0;
const PING_SCALE: f32 = MINIMAP_SIZE / 80.0;

/// KRP's `drawSprite` arguments after the image and rectangle.
#[derive(Clone, Copy)]
pub struct Shadow {
    pub on: bool,
    pub shift: f64,
    pub scale_y: f64,
    pub h_off: f64,
}

pub const NO_SHADOW: Shadow = Shadow {
    on: false,
    shift: 0.0,
    scale_y: 0.0,
    h_off: 0.0,
};

const fn shadow(shift: f64, scale_y: f64, h_off: f64) -> Shadow {
    Shadow {
        on: true,
        shift,
        scale_y,
        h_off,
    }
}

/// KRP's `cachedShadows`: one shadow per sprite, sized by the first call.
/// KRP keys it by sprite index, which a flipped copy shares with its
/// original, so a mirrored sprite can get the unmirrored one's shadow.
#[derive(Default)]
pub struct Shadows {
    first: std::collections::HashMap<macroquad::miniquad::TextureId, (f32, f32, bool)>,
    pub enabled: bool,
}

impl Shadows {
    pub fn clear(&mut self) {
        self.first.clear();
    }
}

/// KRP `drawSprite`.
#[allow(clippy::too_many_arguments)]
pub fn draw_sprite(
    p: &mut Painter,
    shadows: &mut Shadows,
    img: Option<&Image>,
    dx: f64,
    dy: f64,
    dw: f64,
    dh: f64,
    angle: f64,
    s: Shadow,
) {
    let Some(img) = img else { return };
    if img.width() <= 0.0 {
        return;
    }
    let (dx, dy, dw, dh) = (dx.floor(), dy.floor(), dw.floor(), dh.floor());
    let shift = s.shift.floor();
    p.rotate(angle as f32);
    p.draw_image(img, dx as f32, dy as f32, dw as f32, dh as f32);
    if s.on && shadows.enabled {
        p.set_alpha(1.0);
        p.translate(0.0, shift as f32);
        let id = img.tex.raw_miniquad_id();
        let first =
            shadows
                .first
                .entry(id)
                .or_insert((dw as f32, (dh + s.h_off) as f32, img.flip_x));
        let (w, h, flip_x) = *first;
        if w != 0.0 {
            let intensity = if (s.scale_y - 0.5).abs() < f64::EPSILON {
                SHADOW_INTENSITY
            } else {
                SHADOW_INTENSITY * 0.75
            };
            let shadow_img = Image {
                flip_x,
                flip_y: !img.flip_y,
                ..img.clone()
            };
            p.draw_image_colored(
                &shadow_img,
                dx as f32,
                (dy + dh) as f32,
                w,
                h * s.scale_y as f32,
                Color::new(0.0, 0.0, 0.0, intensity),
                false,
            );
        }
        p.rotate(-angle as f32);
        p.translate(0.0, -shift as f32);
    }
}

impl Game {
    /// KRP `doGame`.
    pub(super) fn do_game(&mut self, gfx: &mut Gfx, delta: f64) {
        self.fx.shake.update();
        let me = self.me();
        let (mx, my) = (me.x, me.y);
        self.start_x = mx - self.max_w / 2.0 - self.fx.shake.x
            + self.target.d_offset * (self.target.f + PI).cos();
        self.start_y = my - 20.0 - self.max_h / 2.0 - self.fx.shake.y
            + self.target.d_offset * (self.target.f + PI).sin();
        gfx.shadows.enabled = self.settings.show_shadows;
        self.fx.show_particles = self.settings.show_particles;

        // drawBackground
        let filler = self.sprites.dark_filler.clone();
        draw_sprite(
            &mut gfx.painter,
            &mut gfx.shadows,
            filler.as_ref(),
            0.0,
            0.0,
            self.max_w,
            self.max_h,
            0.0,
            NO_SHADOW,
        );
        self.draw_map(gfx, 0);
        self.draw_map(gfx, 1);
        self.draw_sprays(gfx);
        let view = self.view();
        let my_height = self.me().height;
        if let Some(m) = &self.map {
            self.fx.update_particles(
                &mut gfx.painter,
                delta,
                0,
                &view,
                &m.walls,
                my_height,
                &self.sprites.particles,
            );
        }
        self.draw_game_objects(gfx, delta);
        self.update_bullets(gfx, delta);
        if let Some(m) = &self.map {
            self.fx.update_particles(
                &mut gfx.painter,
                delta,
                1,
                &view,
                &m.walls,
                my_height,
                &self.sprites.particles,
            );
        }
        self.draw_map(gfx, 2);
        self.draw_player_names(gfx);
        self.draw_edge_shader(gfx);
        self.draw_game_lights(gfx, delta);
        let start = (self.start_x, self.start_y);
        self.anim.update_texts(&mut gfx.painter, delta, start);
        self.anim.update_notifications(&mut gfx.painter, delta);
        self.minimap_counter -= 1;
        if self.minimap_counter <= 0 && self.game_start {
            self.minimap_counter = super::MINIMAP_EVERY;
            self.draw_minimap(gfx);
        }
    }

    /// KRP `drawMap(layer)`, with the pickups on layer 0.
    fn draw_map(&mut self, gfx: &mut Gfx, layer: u8) {
        let Some(m) = &self.map else { return };
        let view = self.view();
        let (sx, sy) = (self.start_x, self.start_y);
        let ts = m.tile_scale;
        let mut restore = gfx.restorer();
        for t in &m.tiles {
            match layer {
                0 if !t.wall && view.can_see(t.x - sx, t.y - sy, ts, ts) => {
                    if let Some((img, w, h)) = gfx.tiles.floor(t, &self.sprites, &mut restore) {
                        gfx.painter.invalidate();
                        draw_sprite(
                            &mut gfx.painter,
                            &mut gfx.shadows,
                            Some(&img),
                            t.x - sx,
                            t.y - sy,
                            f64::from(w),
                            f64::from(h),
                            0.0,
                            NO_SHADOW,
                        );
                    }
                }
                1 if t.wall
                    && !t.bottom
                    && view.can_see(t.x - sx, t.y - sy + ts * 0.5, ts, ts * 0.75) =>
                {
                    draw_sprite(
                        &mut gfx.painter,
                        &mut gfx.shadows,
                        self.sprites
                            .wall_segments
                            .get(t.sprite_index)
                            .and_then(Option::as_ref),
                        t.x - sx,
                        t.y + (ts / 2.0).round() - sy,
                        ts,
                        ts / 2.0,
                        0.0,
                        shadow(-(t.scale / 2.0), 0.5, t.scale),
                    );
                }
                2 if t.wall && view.can_see(t.x - sx, t.y - sy - ts * 0.5, ts, ts) => {
                    if let Some(img) = gfx.tiles.wall(t, &self.sprites, &mut restore) {
                        gfx.painter.invalidate();
                        draw_sprite(
                            &mut gfx.painter,
                            &mut gfx.shadows,
                            Some(&img),
                            t.x - sx,
                            (t.y - ts / 2.0 - sy).round(),
                            ts,
                            ts,
                            0.0,
                            NO_SHADOW,
                        );
                    }
                }
                _ => {}
            }
        }
        if layer == 0 {
            for pk in &m.pickups {
                if !pk.active || !view.can_see(pk.x - sx, pk.y - sy, 0.0, 0.0) {
                    continue;
                }
                let img = if pk.kind == "healthpack" {
                    self.sprites.healthpack.as_ref()
                } else {
                    self.sprites.lootcrate.as_ref()
                };
                draw_sprite(
                    &mut gfx.painter,
                    &mut gfx.shadows,
                    img,
                    pk.x - pk.scale / 2.0 - sx,
                    pk.y - pk.scale / 2.0 - sy,
                    pk.scale,
                    pk.scale,
                    0.0,
                    shadow(0.0, 0.5, 0.0),
                );
            }
        }
    }

    /// KRP `drawSprays` (with `cacheSpray` folded in).
    fn draw_sprays(&mut self, gfx: &mut Gfx) {
        if !self.settings.show_sprays {
            return;
        }
        for sp in &self.sprays {
            if !sp.active {
                continue;
            }
            let Some(tex) = gfx.remote.get(&sp.src) else {
                continue;
            };
            let img = Image::sprite(tex);
            gfx.painter.save();
            gfx.painter.set_alpha(sp.alpha as f32);
            gfx.painter.draw_image(
                &img,
                (sp.x - self.start_x) as f32,
                (sp.y - self.start_y) as f32,
                sp.scale as f32,
                sp.scale as f32,
            );
            gfx.painter.restore();
        }
    }

    /// KRP `drawGameObjects`: players, barrels and flags by depth.
    fn draw_game_objects(&mut self, gfx: &mut Gfx, delta: f64) {
        enum Obj {
            Player(usize),
            Clutter(usize),
            Flag(usize),
        }
        let mut objs: Vec<(f64, Obj)> = Vec::new();
        for (i, p) in self.players.iter().enumerate() {
            objs.push((p.y, Obj::Player(i)));
        }
        if let Some(m) = &self.map {
            for (i, c) in m.world.clutter.iter().enumerate() {
                objs.push((c.y, Obj::Clutter(i)));
            }
            for (i, f) in m.flags.iter().enumerate() {
                objs.push((f.y, Obj::Flag(i)));
            }
        }
        objs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let (sx, sy) = (self.start_x, self.start_y);
        let view = self.view();
        let my_team = self.me().team.clone();
        for (_, o) in objs {
            match o {
                Obj::Player(i) => self.draw_player(gfx, i, delta),
                Obj::Clutter(i) => {
                    let Some(m) = &self.map else { continue };
                    let c = &m.world.clutter[i];
                    if c.active && view.can_see(c.x - sx, c.y - sy, c.w, c.h) {
                        draw_sprite(
                            &mut gfx.painter,
                            &mut gfx.shadows,
                            self.sprites
                                .clutter
                                .get(usize::from(c.i))
                                .and_then(Option::as_ref),
                            c.x - sx,
                            c.y - c.h - sy,
                            c.w,
                            c.h,
                            0.0,
                            Shadow {
                                on: c.s,
                                ..shadow(0.0, 0.5, 0.0)
                            },
                        );
                    }
                }
                Obj::Flag(i) => {
                    let Some(m) = &mut self.map else { continue };
                    let f = &mut m.flags[i];
                    f.ac -= 1;
                    if f.ac <= 0 {
                        f.ac = 5;
                        f.ai += 1;
                        if f.ai > 2 {
                            f.ai = 0;
                        }
                    }
                    let idx = f.ai + if f.team == my_team { 0 } else { 3 };
                    draw_sprite(
                        &mut gfx.painter,
                        &mut gfx.shadows,
                        self.sprites.flags.get(idx).and_then(Option::as_ref),
                        f.x - f.w / 2.0 - sx,
                        f.y - f.h - sy,
                        f.w,
                        f.h,
                        0.0,
                        shadow(0.0, 0.5, 0.0),
                    );
                }
            }
        }
        gfx.painter.set_alpha(1.0);
    }

    /// KRP `getPlayerSprite`.
    fn body_sprite(&self, class: usize, angle: i32, anim: usize) -> Option<Image> {
        let c = self.sprites.classes.get(class)?;
        let list = match angle {
            90 => &c.left,
            180 => &c.up,
            270 => &c.right,
            _ => &c.down,
        };
        list.get(anim).cloned().flatten()
    }

    /// KRP `getHatSprite` / `getShirtSprite`: run-time images, used once
    /// every one of their directions has loaded.
    fn wearable_sprite(
        gfx: &mut Gfx,
        kind: &str,
        w: &super::model::Wearable,
        angle: i32,
    ) -> Option<Image> {
        let base = format!("/images/{kind}/{}", w.id);
        let mut need = vec![format!("{base}/d.png")];
        if w.left {
            need.push(format!("{base}/l.png"));
        }
        if w.up {
            need.push(format!("{base}/u.png"));
        }
        let mut all = true;
        for n in &need {
            if gfx.remote.get(n).is_none() {
                all = false;
            }
        }
        if !all {
            return None;
        }
        let get = |gfx: &mut Gfx, f: &str| {
            gfx.remote
                .get(&format!("{base}/{f}.png"))
                .map(Image::sprite)
        };
        if w.left && angle == 90 {
            get(gfx, "l")
        } else if w.up && angle == 180 {
            get(gfx, "u")
        } else if w.left && angle == 270 {
            get(gfx, "l").map(|i| i.flipped())
        } else {
            get(gfx, "d")
        }
    }

    /// KRP `getWeaponSprite`, camo included once it has loaded.
    fn weapon_sprite(
        &self,
        gfx: &mut Gfx,
        weapon_index: usize,
        camo: f64,
        angle: i32,
    ) -> Option<Image> {
        let ws = self.sprites.weapons.get(weapon_index)?;
        let base = match angle {
            90 => ws.left.clone(),
            180 => ws.up.clone(),
            270 => ws.right.clone(),
            _ => ws.down.clone(),
        }?;
        if camo < 0.0 {
            return Some(base);
        }
        let key = (weapon_index, camo as i64, angle);
        if let Some(img) = gfx.weapon_cache.get(&key) {
            return Some(img.clone());
        }
        let Some(tex) = gfx
            .remote
            .get(&format!("/images/camos/{}.png", camo as i64 + 1))
        else {
            return Some(base);
        };
        let camo_img = Image::sprite(tex);
        let (w, h) = (camo_img.width(), camo_img.height());
        let canvas = Canvas::new(w as u32, h as u32);
        canvas.begin(&mut gfx.off);
        gfx.off.draw_image(&base, 0.0, 0.0, w, h);
        gfx.off.set_blend(Blend::Atop);
        gfx.off.set_alpha(0.75);
        let camo_img = if base.flip_x {
            camo_img.flipped()
        } else {
            camo_img
        };
        gfx.off.draw_image(&camo_img, 0.0, 0.0, w, h);
        gl_use_default_material();
        gfx.set_main_camera();
        gfx.painter.invalidate();
        let img = canvas.image();
        gfx.weapon_canvases.push(canvas);
        gfx.weapon_cache.insert(key, img.clone());
        Some(img)
    }

    /// KRP `drawPlayer`: the player is put together on a 300x500 canvas,
    /// which is then drawn onto the game.
    // 0.318 and 0.682 are KRP's leg and torso shares of the height.
    #[allow(clippy::approx_constant)]
    fn draw_player(&mut self, gfx: &mut Gfx, i: usize, _delta: f64) {
        let me = self.me;
        let plr = self.players[i].clone();
        if plr.dead || (Some(plr.index) != me && !plr.on_screen) {
            return;
        }
        let snapped = snap_angle(plr.angle);
        let weapon_angle = (PI / 180.0 * plr.angle) as f32;
        let screen_x = plr.x - self.start_x;
        let mut screen_y = plr.y - plr.jump_y - self.start_y;
        if plr.anim_index == 1 {
            screen_y -= 3.0;
        }
        let weapon = plr.weapon().cloned();
        let weapon_img = weapon
            .as_ref()
            .and_then(|w| self.weapon_sprite(gfx, w.spec.weapon_index, w.camo, snapped));
        let arm = self
            .sprites
            .classes
            .get(plr.class_index)
            .and_then(|c| c.arm.clone());
        let lower = self.body_sprite(
            plr.class_index,
            snapped,
            (plr.anim_index + 1).max(0) as usize,
        );
        let upper = self.body_sprite(plr.class_index, snapped, 0);
        let shirt = match &plr.account.shirt {
            Some(s) if plr.class_index != 8 => Self::wearable_sprite(gfx, "shirts", s, snapped),
            _ => None,
        };
        let hat = match &plr.account.hat {
            Some(h) => Self::wearable_sprite(gfx, "hats", h, snapped),
            None => self
                .sprites
                .classes
                .get(plr.class_index)
                .and_then(|c| match snapped {
                    90 => c.head_left.clone(),
                    180 => c.head_up.clone(),
                    270 => c.head_right.clone(),
                    _ => c.head_down.clone(),
                }),
        };

        if gfx.player_canvases.is_empty() {
            gfx.player_canvases.insert(
                0,
                Canvas::new(PLAYER_CANVAS_W as u32, PLAYER_CANVAS_H as u32),
            );
        }
        let canvas = &gfx.player_canvases[&0];
        let p = &mut gfx.off;
        let sh = &mut gfx.shadows;
        canvas.begin(p);
        p.save();
        p.set_alpha(0.9);
        p.translate(PLAYER_CANVAS_W / 2.0, PLAYER_CANVAS_H / 2.0);
        let draw_arm = |p: &mut Painter, sh: &mut Shadows| {
            draw_sprite(p, sh, arm.as_ref(), 0.0, 0.0, 8.0, 32.0, 0.0, NO_SHADOW);
        };
        if let (Some(w), Some(wi)) = (&weapon, &weapon_img) {
            if !w.front {
                p.save();
                p.translate(0.0, -w.spec.y_offset as f32);
                p.rotate(weapon_angle);
                p.translate(0.0, w.spec.hold_dist as f32);
                draw_sprite(
                    p,
                    sh,
                    Some(wi),
                    -(w.width / 2.0),
                    0.0,
                    w.width,
                    w.length,
                    0.0,
                    NO_SHADOW,
                );
                p.translate(0.0, (-w.spec.hold_dist + 6.0) as f32);
                if arm.is_some() {
                    p.translate(3.0, -10.0);
                    draw_arm(p, sh);
                    p.translate(-16.0, -8.0);
                    draw_arm(p, sh);
                    p.restore();
                }
            }
        }
        p.set_alpha(1.0);
        let jump_shift = plr.jump_y * 1.5;
        draw_sprite(
            p,
            sh,
            lower.as_ref(),
            -(plr.width / 2.0),
            -(plr.height * 0.318),
            plr.width,
            plr.height * 0.318,
            0.0,
            shadow(jump_shift, 0.5, 0.0),
        );
        draw_sprite(
            p,
            sh,
            upper.as_ref(),
            -(plr.width / 2.0),
            -plr.height,
            plr.width,
            plr.height * 0.681_999_999_999_999_9,
            0.0,
            shadow(jump_shift + plr.height * 0.477, 0.5, 0.0),
        );
        if shirt.is_some() {
            p.set_alpha(0.9);
            draw_sprite(
                p,
                sh,
                shirt.as_ref(),
                -(plr.width / 2.0),
                -plr.height,
                plr.width,
                plr.height * 0.681_999_999_999_999_9,
                0.0,
                shadow(jump_shift + plr.height * 0.477, 0.5, 0.0),
            );
            p.set_alpha(1.0);
        }
        let hat_scale = plr.width * 0.833;
        draw_sprite(
            p,
            sh,
            hat.as_ref(),
            -(hat_scale / 2.0),
            -(plr.height + hat_scale * 0.045),
            hat_scale,
            hat_scale,
            0.0,
            Shadow {
                on: false,
                ..shadow(0.0, 0.5, 0.0)
            },
        );
        if let (Some(w), Some(wi)) = (&weapon, &weapon_img) {
            p.set_alpha(0.9);
            if w.front {
                p.save();
                p.translate(0.0, -w.spec.y_offset as f32);
                p.rotate(weapon_angle);
                p.translate(0.0, w.spec.hold_dist as f32);
                draw_sprite(
                    p,
                    sh,
                    Some(wi),
                    -(w.width / 2.0),
                    0.0,
                    w.width,
                    w.length,
                    0.0,
                    NO_SHADOW,
                );
                p.translate(0.0, (-w.spec.hold_dist + 10.0) as f32);
                if arm.is_some() {
                    if snapped == 270 {
                        p.restore();
                        p.save();
                        p.translate(-4.0, (-w.spec.y_offset + 8.0) as f32);
                        p.rotate(weapon_angle);
                        draw_arm(p, sh);
                    } else if snapped == 90 {
                        p.restore();
                        p.save();
                        p.translate(0.0, -w.spec.y_offset as f32);
                        p.rotate(weapon_angle);
                        draw_arm(p, sh);
                    } else {
                        p.translate(10.0, -13.0);
                        p.rotate(0.7);
                        draw_arm(p, sh);
                        p.rotate(-0.7);
                        p.translate(-28.0, -1.0);
                        p.rotate(-0.25);
                        draw_arm(p, sh);
                        p.rotate(0.25);
                    }
                    p.restore();
                }
            }
        }
        p.restore();
        gl_use_default_material();
        gfx.set_main_camera();
        gfx.painter.invalidate();

        // The spawn-protection tint is KRP's source-atop fill over the
        // whole player canvas; here it is applied while drawing it back.
        let img = gfx.player_canvases[&0].image();
        let (dx, dy) = (
            (screen_x - f64::from(PLAYER_CANVAS_W) / 2.0).floor() as f32,
            (screen_y - f64::from(PLAYER_CANVAS_H) / 2.0).floor() as f32,
        );
        if plr.is_spawn_protected {
            let c = if plr.team == self.me().team {
                Color::new(179.0 / 255.0, 231.0 / 255.0, 1.0, 0.5)
            } else {
                Color::new(1.0, 179.0 / 255.0, 179.0 / 255.0, 0.5)
            };
            gfx.painter
                .draw_image_colored(&img, dx, dy, PLAYER_CANVAS_W, PLAYER_CANVAS_H, c, true);
        } else {
            gfx.painter
                .draw_image(&img, dx, dy, PLAYER_CANVAS_W, PLAYER_CANVAS_H);
        }
        // Each player's canvas must reach the screen before the next one
        // reuses it.
        gfx.painter.invalidate();
    }

    /// KRP `updateBullets`: moves, collides and draws every bullet.
    fn update_bullets(&mut self, gfx: &mut Gfx, delta: f64) {
        gfx.painter.set_alpha(1.0);
        let now = self.current_time;
        let me = self.me;
        let view = self.view();
        let (sx, sy) = (self.start_x, self.start_y);
        let targets_owned: Vec<Player> = self.players.clone();
        let targets: Vec<_> = targets_owned
            .iter()
            .map(|p| vertix_sim::projectile::Target {
                index: p.index,
                team: &p.team,
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
                jump_y: p.jump_y,
                on_screen: p.on_screen,
                dead: p.dead,
                spawn_protected: p.is_spawn_protected,
            })
            .collect();
        let empty: [vertix_sim::projectile::Target<'_>; 0] = [];
        let mut hit_effects: Vec<(Hit, f64, f64, f64, u32)> = Vec::new();
        let mut dust: Vec<(f64, f64)> = Vec::new();
        for b in &mut self.bullets {
            let was_active = b.p.active;
            if let Some(m) = &self.map {
                // KRP's client only checks player hits for its own bullets.
                let who: &[vertix_sim::projectile::Target<'_>] = if Some(b.p.owner.index) == me {
                    &targets
                } else {
                    &empty
                };
                b.p.update(delta, now, &m.world.clutter, &m.world.tiles, who);
            }
            if was_active {
                // `hits` keeps the last update's hits once a bullet is
                // inactive, so only an update that ran may show them (KRP
                // makes its effects once, inside hitSomething).
                for h in &b.p.hits {
                    hit_effects.push((*h, b.p.x, b.p.y, b.p.dir, b.p.sprite_index));
                }
                if b.p.sprite_index == 1 {
                    b.dust_timer -= delta;
                    if b.dust_timer <= 0.0 {
                        dust.push((b.p.x, b.p.y));
                        b.dust_timer = 20.0;
                    }
                }
            } else if b.trail_alpha > 0.0 {
                b.trail_alpha -= delta * 0.001;
                if b.trail_alpha <= 0.0 {
                    b.trail_alpha = 0.0;
                }
            }
            let p = &mut gfx.painter;
            if b.p.active {
                let x = b.p.x - sx;
                let y = b.p.y - sy;
                if view.can_see(x, y, b.p.height, b.p.height) {
                    p.save();
                    p.translate(x as f32, y as f32);
                    let img = self
                        .sprites
                        .bullets
                        .get(b.p.sprite_index as usize)
                        .and_then(Option::as_ref);
                    if b.p.sprite_index == 2 {
                        p.set_blend(Blend::Additive);
                        p.set_alpha(0.3);
                        draw_sprite(
                            p,
                            &mut gfx.shadows,
                            img,
                            -(b.glow_width / 2.0),
                            -(b.glow_height / 2.0) + b.p.height / 2.0,
                            b.glow_width,
                            b.glow_height,
                            b.p.dir - PI / 2.0,
                            NO_SHADOW,
                        );
                    } else {
                        draw_sprite(
                            p,
                            &mut gfx.shadows,
                            img,
                            -(b.p.width / 2.0),
                            0.0,
                            b.p.width,
                            b.p.height + 8.0,
                            b.p.dir - PI / 2.0,
                            NO_SHADOW,
                        );
                    }
                    p.restore();
                }
            }
            if self.settings.show_trails && b.trail_alpha > 0.0 {
                let a = vec2(
                    (b.p.start_x - sx).round() as f32,
                    (b.p.start_y - sy).round() as f32,
                );
                let c = vec2((b.p.x - sx).round() as f32, (b.p.y - sy).round() as f32);
                p.gradient_line(
                    a,
                    c,
                    b.trail_width as f32,
                    Color::new(1.0, 1.0, 1.0, 0.0),
                    Color::new(1.0, 1.0, 1.0, b.trail_alpha as f32),
                );
            }
        }
        for (h, _x, _y, dir, sprite) in hit_effects {
            if sprite == 2 {
                continue;
            }
            match h {
                Hit::Surface { x, y, flip_y } => {
                    let spread = PI / random_int(5, 7) as f64;
                    self.fx
                        .particle_cone(10, x, y, dir + PI, spread, 0.5, 16.0, 2, flip_y);
                }
                Hit::Player { index } => {
                    if let Some(pl) = self.find(index) {
                        let (px, py, ph, pj) = (pl.x, pl.y, pl.height, pl.jump_y);
                        let spread = PI / random_int(5, 7) as f64;
                        self.fx.particle_cone(
                            12,
                            px,
                            py - ph / 2.0 - pj,
                            dir + PI,
                            spread,
                            0.5,
                            16.0,
                            0,
                            true,
                        );
                        self.fx.liquid(px, py, 4);
                    }
                }
            }
        }
        for (x, y) in dust {
            self.fx.still_dust(x, y, true);
        }
    }

    /// KRP `drawPlayerNames`: names, clan tags, ranks and health bars.
    fn draw_player_names(&mut self, gfx: &mut Gfx) {
        let me = self.me;
        let my_team = self.me().team.clone();
        gfx.painter.set_alpha(1.0);
        for plr in &self.players {
            if plr.dead || (Some(plr.index) != me && !plr.on_screen) {
                continue;
            }
            let size = plr.height / 3.2;
            let bar_w = (plr.max_health / 100.0 * 100.0).min(200.0);
            let x = plr.x - self.start_x;
            let mut y = plr.y - plr.jump_y - plr.name_y_offset - self.start_y;
            if let Some(h) = &plr.account.hat {
                y -= h.name_y;
            }
            let color = if plr.team == my_team {
                TEAM_BLUE
            } else {
                TEAM_RED
            };
            if self.settings.show_names {
                let mut restore = gfx.restorer();
                let name = gfx.text.render_shaded(
                    &plr.name,
                    (size * TEXT_SIZE_MULT) as f32,
                    "#ffffff",
                    5,
                    false,
                    &mut restore,
                );
                gfx.painter.invalidate();
                let ny = y as f32 - plr.height as f32 * 1.4;
                gfx.painter.draw_image(
                    &name.image,
                    x as f32 - name.width / 2.0,
                    ny - name.height / 2.0,
                    name.width,
                    name.height,
                );
                if plr.logged_in {
                    let rank = gfx.text.render_shaded(
                        &plr.account.rank.to_string(),
                        (size * 1.6 * TEXT_SIZE_MULT) as f32,
                        "#ffffff",
                        6,
                        false,
                        &mut restore,
                    );
                    gfx.painter.invalidate();
                    gfx.painter.draw_image(
                        &rank.image,
                        x as f32 - name.width / 2.0 - rank.width - TEXT_SIZE_MULT as f32 * 5.0,
                        ny - (rank.height - name.height / 2.0),
                        rank.width,
                        rank.height,
                    );
                }
                if !plr.account.clan.is_empty() {
                    let clan = gfx.text.render_shaded(
                        &format!(" [{}]", plr.account.clan),
                        (size * TEXT_SIZE_MULT) as f32,
                        color,
                        5,
                        false,
                        &mut restore,
                    );
                    gfx.painter.invalidate();
                    gfx.painter.draw_image(
                        &clan.image,
                        x as f32 + name.width / 2.0,
                        ny - name.height / 2.0,
                        clan.width,
                        name.height,
                    );
                }
            }
            let frac = plr.health / plr.max_health;
            gfx.painter.fill_rect(
                (x - (bar_w / 2.0) * frac) as f32,
                (y - plr.height * 1.16) as f32,
                (frac * bar_w) as f32,
                10.0,
                hex(color),
            );
        }
    }

    /// KRP `drawEdgeShader`. KRP builds its gradient once, around wherever
    /// the player was on screen the first time, and keeps it.
    fn draw_edge_shader(&mut self, gfx: &mut Gfx) {
        if gfx.edge.is_none() {
            let me = self.me();
            let cx = me.x - self.start_x;
            let cy = me.y - self.start_y;
            let r = self.max_w / 2.0;
            let (w, h) = (
                (self.max_w / 4.0).ceil().max(1.0) as u16,
                (self.max_h / 4.0).ceil().max(1.0) as u16,
            );
            let mut bytes = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
            for py in 0..h {
                for px in 0..w {
                    let x = (f64::from(px) + 0.5) * 4.0;
                    let y = (f64::from(py) + 0.5) * 4.0;
                    let t = ((x - cx).hypot(y - cy) / r).min(1.0);
                    bytes.extend_from_slice(&[0, 0, 0, (t * 0.4 * 255.0).round() as u8]);
                }
            }
            let tex = Texture2D::from_rgba8(w, h, &bytes);
            tex.set_filter(FilterMode::Linear);
            gfx.edge = Some(tex);
        }
        if let Some(tex) = &gfx.edge {
            let img = Image::sprite(tex.clone());
            gfx.painter
                .draw_image(&img, 0.0, 0.0, self.max_w as f32, self.max_h as f32);
        }
    }

    /// KRP `drawGameLights`: bullet glows and explosion flashes.
    fn draw_game_lights(&mut self, gfx: &mut Gfx, delta: f64) {
        let Some(light) = self.sprites.light.clone() else {
            return;
        };
        let p = &mut gfx.painter;
        p.set_blend(Blend::Additive);
        p.set_alpha(0.2);
        let view = self.view();
        for b in &self.bullets {
            if !self.settings.show_glows || b.p.sprite_index == 2 || !b.p.active {
                continue;
            }
            let gw = if b.glow_width > 0.0 {
                b.glow_width
            } else {
                (b.p.width * 14.0).min(200.0)
            };
            let gh = if b.glow_height > 0.0 {
                b.glow_height
            } else {
                b.p.height * 2.5
            };
            let lx = b.p.x - self.start_x;
            let ly = b.p.y - self.start_y;
            if !view.can_see(lx, ly, gw, gh) {
                continue;
            }
            p.save();
            p.translate(lx as f32, ly as f32);
            draw_sprite(
                p,
                &mut gfx.shadows,
                Some(&light),
                -(gw / 2.0),
                -(gh / 2.0) + b.p.height / 2.0,
                gw,
                gh,
                b.p.dir - PI / 2.0,
                NO_SHADOW,
            );
            p.restore();
        }
        if self.settings.show_glows {
            p.set_alpha(0.2);
            self.fx
                .update_glows(p, delta, Some(&light), (self.start_x, self.start_y));
        }
        p.set_blend(Blend::Normal);
    }

    /// KRP `drawMiniMap` into the minimap canvas (every fourth frame).
    fn draw_minimap(&mut self, gfx: &mut Gfx) {
        let Some(m) = &self.map else { return };
        let my_team = self.me().team.clone();
        let me = self.me;
        let (gw, gh) = (m.width, m.height);
        if gfx.minimap_base.is_none() {
            gfx.minimap_base = Some(minimap_base(gfx, m, &my_team));
        }
        let canvas = gfx
            .minimap
            .get_or_insert_with(|| Canvas::new(MINIMAP_SIZE as u32, MINIMAP_SIZE as u32));
        let p = &mut gfx.off;
        canvas.begin(p);
        if let Some(base) = &gfx.minimap_base {
            p.draw_image(&base.image(), 0.0, 0.0, MINIMAP_SIZE, MINIMAP_SIZE);
        }
        let s = f64::from(MINIMAP_SIZE);
        for plr in &self.players {
            if !plr.dead
                && plr.on_screen
                && (Some(plr.index) == me || plr.team == my_team || plr.is_boss)
            {
                let c = if Some(plr.index) == me {
                    WHITE
                } else if plr.is_boss {
                    hex("#db4fcd")
                } else {
                    hex(TEAM_BLUE)
                };
                p.fill_circle(
                    (plr.x / gw * s) as f32,
                    (plr.y / gh * s) as f32,
                    PING_SCALE,
                    c,
                );
            }
        }
        for pk in &m.pickups {
            if !pk.active {
                continue;
            }
            // KRP keeps the last fill colour for unknown pickup types.
            let c = if pk.kind == "lootcrate" {
                hex("#ffd100")
            } else {
                hex("#5ed951")
            };
            p.fill_circle(
                (pk.x / gw * s) as f32,
                (pk.y / gh * s) as f32,
                PING_SCALE,
                c,
            );
        }
        gl_use_default_material();
        gfx.set_main_camera();
        gfx.painter.invalidate();
    }
}

/// KRP `getCachedMiniMap`: walls at 10% white, hardpoints in team colour.
/// KRP divides both tile sides by the map width; kept.
fn minimap_base(gfx: &mut Gfx, m: &MapState, my_team: &str) -> Canvas {
    let s = f64::from(MINIMAP_SIZE);
    let side = (m.tile_scale * 1.08 / m.width * s) as f32;
    let walls = Canvas::new(MINIMAP_SIZE as u32, MINIMAP_SIZE as u32);
    walls.begin(&mut gfx.off);
    for t in m.tiles.iter().filter(|t| t.wall) {
        gfx.off.fill_rect(
            (t.x / m.width * s) as f32,
            (t.y / m.height * s) as f32,
            side,
            side,
            WHITE,
        );
    }
    let out = Canvas::new(MINIMAP_SIZE as u32, MINIMAP_SIZE as u32);
    out.begin(&mut gfx.off);
    gfx.off.set_alpha(0.1);
    gfx.off
        .draw_image(&walls.image(), 0.0, 0.0, MINIMAP_SIZE, MINIMAP_SIZE);
    gfx.off.set_alpha(1.0);
    for t in m.tiles.iter().filter(|t| t.hard_point) {
        let c = if t.obj_team == my_team {
            TEAM_BLUE
        } else {
            TEAM_RED
        };
        gfx.off.fill_rect(
            (t.x / m.width * s) as f32,
            (t.y / m.height * s) as f32,
            side,
            side,
            hex(c),
        );
    }
    gl_use_default_material();
    // Hand the wall canvas to the cache so it lives until drawn.
    gfx.weapon_canvases.push(walls);
    out
}

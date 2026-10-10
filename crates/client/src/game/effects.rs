//! Particles, flash glows and screen shake: `visual/particle.ts`,
//! `visual/flash.ts` and `visual/shake.ts`, frame for frame.

#![forbid(unsafe_code)]

use std::f64::consts::PI;

use crate::game::rand::{random_float, random_int};
use crate::gfx::{Image, Painter};

#[derive(Debug, Clone, Default)]
pub struct Particle {
    rotation: f64,
    init_scale: f64,
    scale: f64,
    dir: f64,
    init_speed: f64,
    speed: f64,
    pub y: f64,
    pub x: f64,
    pub active: bool,
    layer: i64,
    sprite_index: usize,
    alpha: f64,
    fade_speed: f64,
    force_show: bool,
    check_collisions: bool,
    max_duration: f64,
    duration: f64,
}

/// The wall rectangles particles stop at (`checkInWall`).
pub struct WallBox {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

impl Particle {
    fn update(&mut self, delta: f64, walls: &[WallBox], player_height: f64) {
        if !self.active {
            return;
        }
        if self.max_duration > 0.0 {
            self.duration += delta;
            let mut s = 1.0 - self.duration / self.max_duration;
            if s < 0.0 {
                s = 0.0;
            }
            self.scale = self.init_scale * s;
            if self.scale < 1.0 {
                self.active = false;
            }
            self.speed = self.init_speed * s;
            if self.speed <= 0.01 {
                self.speed = 0.0;
            } else {
                self.x += self.speed * delta * self.dir.cos();
                self.y += self.speed * delta * self.dir.sin();
            }
            if self.duration >= self.max_duration {
                self.active = false;
            }
        }
        if self.alpha > 0.0 {
            self.alpha -= self.fade_speed * delta;
        }
        if self.alpha <= 0.0 {
            self.alpha = 0.0;
            self.active = false;
        }
        if self.check_collisions {
            for t in walls {
                if self.x >= t.x
                    && self.x <= t.x + t.scale
                    && self.y > t.y
                    && self.y < t.y + t.scale - player_height
                {
                    self.active = false;
                }
            }
        }
    }

    fn draw(&self, p: &mut Painter, sprites: &[Option<Image>], start: (f64, f64)) {
        let Some(Some(img)) = sprites.get(self.sprite_index) else {
            return;
        };
        if !self.active {
            return;
        }
        p.set_alpha(self.alpha as f32);
        let s = self.scale as f32;
        if self.rotation == 0.0 {
            p.draw_image(
                img,
                (self.x - start.0 - self.scale / 2.0) as f32,
                (self.y - start.1 - self.scale / 2.0) as f32,
                s,
                s,
            );
        } else {
            p.save();
            p.translate((self.x - start.0) as f32, (self.y - start.1) as f32);
            p.rotate(self.rotation as f32);
            p.draw_image(img, -s / 2.0, -s / 2.0, s, s);
            p.restore();
        }
    }
}

#[derive(Debug, Clone, Default)]
struct FlashGlow {
    init_scale: f64,
    scale: f64,
    y: f64,
    x: f64,
    active: bool,
    max_duration: f64,
    duration: f64,
}

/// What the effects need to know about the view.
pub struct View {
    pub start_x: f64,
    pub start_y: f64,
    pub max_w: f64,
    pub max_h: f64,
}

impl View {
    /// KRP `canSee`.
    #[must_use]
    pub fn can_see(&self, x: f64, y: f64, w: f64, h: f64) -> bool {
        x + w > 0.0 && y + h > 0.0 && x < self.max_w && y < self.max_h
    }
}

#[derive(Debug, Clone, Default)]
pub struct Shake {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub dir: f64,
}

impl Shake {
    /// KRP `screenShake`.
    pub fn start(&mut self, scale: f64, dir: f64) {
        if self.scale < scale {
            self.scale = scale;
            self.dir = dir;
        }
    }

    /// KRP `updateScreenShake`: halves every frame.
    pub fn update(&mut self) {
        if self.scale > 0.0 {
            self.x = self.scale * self.dir.cos();
            self.y = self.scale * self.dir.sin();
            self.scale *= 0.5;
            if self.scale <= 0.1 {
                self.scale = 0.0;
            }
        }
    }
}

pub struct Effects {
    particles: Vec<Particle>,
    particle_index: usize,
    glows: Vec<FlashGlow>,
    glow_index: usize,
    pub shake: Shake,
    /// `settings.showParticles`
    pub show_particles: bool,
}

const MAX_SHAKE_DIST: f64 = 2000.0;
const MAX_EXPLOSION_DURATION: f64 = 400.0;
const MAX_SHAKE: f64 = 9.0;
const LIQUID_SPREAD: f64 = 35.0;

impl Effects {
    #[must_use]
    pub fn new() -> Self {
        Self {
            particles: vec![Particle::default(); 700],
            particle_index: 0,
            glows: vec![FlashGlow::default(); 30],
            glow_index: 0,
            shake: Shake::default(),
            show_particles: true,
        }
    }

    fn ready(&mut self) -> &mut Particle {
        self.particle_index += 1;
        if self.particle_index >= self.particles.len() {
            self.particle_index = 0;
        }
        &mut self.particles[self.particle_index]
    }

    /// KRP `updateParticles(delta, layer)`: updates and draws one layer.
    pub fn update_particles(
        &mut self,
        p: &mut Painter,
        delta: f64,
        layer: i64,
        view: &View,
        walls: &[WallBox],
        player_height: f64,
        sprites: &[Option<Image>],
    ) {
        let show = self.show_particles;
        for part in &mut self.particles {
            if (show || part.force_show)
                && part.active
                && view.can_see(
                    part.x - view.start_x,
                    part.y - view.start_y,
                    part.scale,
                    part.scale,
                )
            {
                if layer == part.layer {
                    part.update(delta, walls, player_height);
                    part.draw(p, sprites, (view.start_x, view.start_y));
                }
            } else {
                part.active = false;
            }
        }
        p.set_alpha(1.0);
    }

    /// KRP `particleCone`.
    #[allow(clippy::too_many_arguments)]
    pub fn particle_cone(
        &mut self,
        count: usize,
        x: f64,
        y: f64,
        dir: f64,
        spread: f64,
        speed: f64,
        scale: f64,
        sprite_index: usize,
        add_bullet_hole: bool,
    ) {
        if !self.show_particles {
            return;
        }
        for i in 0..count {
            let t = self.ready();
            t.force_show = false;
            t.check_collisions = false;
            t.x = x;
            t.y = y;
            t.rotation = 0.0;
            t.alpha = 1.0;
            t.speed = 0.0;
            t.fade_speed = 0.0;
            t.init_speed = 0.0;
            t.init_scale = random_float(3.0, 9.0);
            t.sprite_index = 0;
            t.max_duration = -1.0;
            t.duration = 0.0;
            if i == 0 && sprite_index == 2 && add_bullet_hole {
                t.sprite_index = 3;
                t.layer = 0;
            } else {
                t.dir = dir + random_float(-spread, spread);
                t.init_scale = scale * random_float(1.5, 1.8);
                t.init_speed = speed * random_float(0.3, 1.3);
                t.max_duration = random_float(0.8, 1.1) * 360.0;
                t.sprite_index = sprite_index;
                t.layer = random_int(0, 1);
            }
            t.scale = t.init_scale;
            t.active = true;
        }
    }

    /// KRP `createLiquid`.
    pub fn liquid(&mut self, x: f64, y: f64, sprite_index: i64) {
        let t = self.ready();
        t.x = x + random_float(-LIQUID_SPREAD, LIQUID_SPREAD);
        t.y = y + random_float(-LIQUID_SPREAD, LIQUID_SPREAD);
        t.init_speed = 0.0;
        t.max_duration = -1.0;
        t.duration = 0.0;
        t.init_scale = random_float(60.0, 150.0);
        t.scale = t.init_scale;
        t.rotation = random_int(0, 5) as f64;
        t.alpha = random_float(0.3, 0.5);
        t.fade_speed = 0.00002;
        t.check_collisions = false;
        t.sprite_index = random_int(sprite_index, sprite_index + 1) as usize;
        t.layer = 0;
        t.force_show = false;
        t.active = true;
    }

    /// KRP `createExplosion`, around the local player at `me`.
    pub fn explosion(&mut self, x: f64, y: f64, scale: f64, me: (f64, f64)) {
        let dist = (x - me.0).hypot(y - me.1);
        if dist <= MAX_SHAKE_DIST {
            // KRP calls getAngle(x, me.x, y, me.y): the arguments are
            // crossed, and kept that way here.
            let dir = (me.1 - me.0).atan2(y - x);
            self.shake
                .start(scale * MAX_SHAKE * (1.0 - dist / MAX_SHAKE_DIST), dir);
        }
        self.smoke_puff(x, y, scale, true, 1.0);
    }

    /// KRP `createSmokePuff`.
    pub fn smoke_puff(&mut self, x: f64, y: f64, scale: f64, hole: bool, speed: f64) {
        self.flash(x, y, scale);
        for i in 0..30 {
            let t = self.ready();
            t.dir = (random_float(-PI, PI) / (PI / 3.0)).round() * (PI / 3.0);
            t.force_show = true;
            t.sprite_index = 2;
            t.check_collisions = true;
            t.alpha = 1.0;
            t.fade_speed = 0.0;
            t.init_speed = 0.0;
            t.max_duration = -1.0;
            t.duration = 0.0;
            t.layer = 1;
            t.rotation = 0.0;
            if i == 0 && hole {
                t.x = x;
                t.y = y;
                t.init_scale = random_float(50.0, 60.0) * scale;
                t.rotation = random_int(0, 5) as f64;
                t.speed = 0.0;
                t.fade_speed = 0.0002;
                t.check_collisions = false;
                t.sprite_index = 6;
                t.layer = 0;
            } else if i <= 10 {
                let d = f64::from(i) * scale;
                t.x = x + d * t.dir.cos();
                t.y = y + d * t.dir.sin();
                t.init_scale = random_float(30.0, 33.0) * scale;
                t.init_speed = (3.0 / t.init_scale) * scale * speed;
                t.max_duration = MAX_EXPLOSION_DURATION * 0.8;
            } else {
                let d = random_float(0.0, 10.0) * scale;
                t.x = x + d * t.dir.cos();
                t.y = y + d * t.dir.sin();
                let r = random_float(0.7, 1.4);
                t.init_scale = scale * 11.0 * r;
                t.init_speed = (((12.0 / t.init_scale) * scale) / r) * speed;
                t.max_duration = MAX_EXPLOSION_DURATION * r;
            }
            t.scale = t.init_scale;
            t.active = true;
        }
    }

    /// KRP `stillDustParticle`.
    pub fn still_dust(&mut self, x: f64, y: f64, force: bool) {
        let t = self.ready();
        t.x = x + random_int(-10, 10) as f64;
        t.y = y;
        t.init_scale = random_float(18.0, 25.0);
        t.init_speed = 0.05;
        t.max_duration = 600.0;
        t.duration = 0.0;
        t.dir = random_float(0.0, PI * 2.0);
        t.rotation = 0.0;
        t.sprite_index = 2;
        t.layer = i64::from(force);
        t.alpha = 1.0;
        t.fade_speed = 0.0;
        t.check_collisions = false;
        t.force_show = force;
        t.active = true;
    }

    /// KRP `createFlash`.
    pub fn flash(&mut self, x: f64, y: f64, scale: f64) {
        self.glow_index += 1;
        if self.glow_index >= self.glows.len() {
            self.glow_index = 0;
        }
        let g = &mut self.glows[self.glow_index];
        g.x = x;
        g.y = y;
        g.scale = 0.0;
        g.init_scale = scale * 220.0;
        g.duration = 0.0;
        g.max_duration = 180.0;
        g.active = true;
    }

    /// KRP `updateFlashGlows`.
    pub fn update_glows(
        &mut self,
        p: &mut Painter,
        delta: f64,
        light: Option<&Image>,
        start: (f64, f64),
    ) {
        for g in &mut self.glows {
            if g.active && g.max_duration > 0.0 {
                g.duration += delta;
                let s = (1.0 - g.duration / g.max_duration).max(0.0);
                g.scale = g.init_scale * s;
                if g.scale < 1.0 || g.duration >= g.max_duration {
                    g.active = false;
                }
            }
            if g.active {
                if let Some(light) = light {
                    let s = g.scale as f32;
                    p.draw_image(
                        light,
                        (g.x - start.0 - g.scale / 2.0) as f32,
                        (g.y - start.1 - g.scale / 2.0) as f32,
                        s,
                        s,
                    );
                }
            }
        }
    }
}

impl Default for Effects {
    fn default() -> Self {
        Self::new()
    }
}

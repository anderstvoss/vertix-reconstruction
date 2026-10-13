//! Floating and big centred texts and the notification stack
//! (`visual/animtext.ts`).

#![forbid(unsafe_code)]

use crate::game::rand::random_int;
use crate::gfx::Painter;
use crate::text::{Baked, Text};

const TEXT_SIZE_MULT: f64 = 0.55;
// KRP computes these once at load, from the 1080 default height.
const BIG_TEXT_SIZE: f64 = (1080.0 / 7.7) * TEXT_SIZE_MULT;
const MED_TEXT_SIZE: f64 = BIG_TEXT_SIZE * 0.85;
const TEXT_GAP: f64 = BIG_TEXT_SIZE * 1.2;
const BIG_TEXT_Y: f64 = 1080.0 / 4.3;
const NOTIFICATION_FADE_SPEED: f64 = 0.003;
const NOTIFICATION_FADE_DELAY: f64 = 800.0;
const ANIM_TEXT_FADE_SPEED: f64 = 0.0025;
const MOVING_TEXT_FADE_DELAY: f64 = 350.0;
const NOTIFICATIONS_SIZE: f64 = TEXT_SIZE_MULT * 80.0;
const NOTIFICATIONS_GAP: f64 = NOTIFICATIONS_SIZE * 1.6;

#[derive(Default)]
struct AnimText {
    scale_speed: f64,
    min_scale: f64,
    max_scale: f64,
    scale: f64,
    y_speed: f64,
    x_speed: f64,
    y: f64,
    x: f64,
    active: bool,
    alpha: f64,
    fade_speed: f64,
    use_start: bool,
    move_delay: f64,
    fade_delay: f64,
    removable: bool,
    big: bool,
    image: Option<Baked>,
}

impl AnimText {
    fn update(&mut self, delta: f64) {
        if !self.active {
            return;
        }
        self.scale += self.scale_speed * delta;
        if self.scale_speed > 0.0 {
            if self.scale >= self.max_scale {
                self.scale = self.max_scale;
                self.scale_speed *= -1.0;
            }
        } else if self.scale < self.min_scale {
            self.scale = self.min_scale;
            self.scale_speed = 0.0;
        }
        if self.move_delay > 0.0 {
            self.move_delay -= delta;
        } else {
            self.x += self.x_speed * delta;
            self.y += self.y_speed * delta;
        }
        if self.fade_delay > 0.0 {
            self.fade_delay -= delta;
        } else {
            self.alpha -= self.fade_speed * delta;
            if self.alpha <= 0.0 {
                self.alpha = 0.0;
                self.active = false;
            }
        }
    }

    fn draw(&self, p: &mut Painter, start: (f64, f64)) {
        let Some(img) = &self.image else {
            return;
        };
        if !self.active {
            return;
        }
        p.set_alpha(self.alpha as f32);
        let w = f64::from(img.width) * self.scale;
        let h = f64::from(img.height) * self.scale;
        let (ox, oy) = if self.use_start { start } else { (0.0, 0.0) };
        p.draw_image(
            &img.image,
            (self.x - ox - w / 2.0) as f32,
            (self.y - oy - h / 2.0) as f32,
            w as f32,
            h as f32,
        );
    }
}

/// What a text needs to know about the screen when it starts.
#[derive(Clone, Copy)]
pub struct Screen {
    pub max_w: f64,
    pub max_h: f64,
    pub view_mult: f64,
}

pub struct AnimTexts {
    texts: Vec<AnimText>,
    notifications: Vec<AnimText>,
    notification_index: usize,
}

impl AnimTexts {
    #[must_use]
    pub fn new() -> Self {
        Self {
            texts: (0..20).map(|_| AnimText::default()).collect(),
            notifications: (0..3).map(|_| AnimText::default()).collect(),
            notification_index: 0,
        }
    }

    /// KRP `showNotification`.
    pub fn notify(&mut self, text: &str, screen: Screen, t: &mut Text, restore: &mut dyn FnMut()) {
        let text = text.to_uppercase();
        self.notification_index = (self.notification_index + 1) % self.notifications.len();
        let size = NOTIFICATIONS_SIZE * screen.view_mult;
        let image = t.render_shaded(&text, size as f32, "#ffffff", 7, true, restore);
        let n = &mut self.notifications[self.notification_index];
        n.alpha = 1.0;
        n.x = screen.max_w / 2.0;
        n.fade_speed = NOTIFICATION_FADE_SPEED;
        n.fade_delay = NOTIFICATION_FADE_DELAY;
        n.scale = 1.0;
        n.scale_speed = 0.005;
        n.min_scale = 1.0;
        n.max_scale = 1.5;
        n.image = Some(image);
        n.active = true;
        self.position_notifications(screen);
    }

    fn position_notifications(&mut self, screen: Screen) {
        let active = self.notifications.iter().filter(|n| n.active).count();
        if active == 0 {
            return;
        }
        // Stable sort by alpha, highest first (KRP sortByAlpha).
        self.notifications.sort_by(|a, b| {
            b.alpha
                .partial_cmp(&a.alpha)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let count = self.notifications.len() as f64;
        let base = screen.max_h - count * NOTIFICATIONS_GAP * screen.view_mult - 100.0;
        let mut row = 0.0;
        for n in &mut self.notifications {
            if n.active {
                n.y = base + NOTIFICATIONS_GAP * screen.view_mult * row;
                row += 1.0;
            }
        }
    }

    /// KRP `updateNotifications`.
    pub fn update_notifications(&mut self, p: &mut Painter, delta: f64) {
        for n in &mut self.notifications {
            if n.active {
                n.update(delta);
                n.draw(p, (0.0, 0.0));
            }
        }
        p.set_alpha(1.0);
    }

    /// KRP `updateAnimTexts`.
    pub fn update_texts(&mut self, p: &mut Painter, delta: f64, start: (f64, f64)) {
        for a in &mut self.texts {
            a.update(delta);
            if a.active {
                a.draw(p, start);
            }
        }
        p.set_alpha(1.0);
    }

    fn deactivate_big(&mut self) -> bool {
        for a in &mut self.texts {
            if !a.active {
                continue;
            }
            if a.removable {
                a.active = false;
            } else if a.big {
                return false;
            }
        }
        true
    }

    /// KRP `deactiveAllAnimTexts`.
    pub fn clear(&mut self) {
        for a in &mut self.texts {
            a.active = false;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        y_speed: f64,
        font_size: f64,
        scale_speed: f64,
        use_start: bool,
        fade_delay: f64,
        move_delay: f64,
        removable: bool,
        color: &str,
        big: bool,
        layers: u32,
        italic: bool,
        screen: Screen,
        t: &mut Text,
        restore: &mut dyn FnMut(),
    ) {
        let Some(slot) = self.texts.iter().position(|a| !a.active) else {
            return;
        };
        let text = text.to_uppercase();
        let size = font_size * screen.view_mult;
        let image = t.render_shaded(&text, size as f32, color, layers, italic, restore);
        self.texts[slot] = AnimText {
            scale_speed,
            min_scale: 1.0,
            max_scale: 1.6,
            scale: 1.0,
            y_speed,
            x_speed: 0.0,
            y,
            x,
            active: true,
            alpha: 1.0,
            fade_speed: ANIM_TEXT_FADE_SPEED,
            use_start,
            move_delay,
            fade_delay,
            removable,
            big,
            image: Some(image),
        };
    }

    /// KRP `startBigAnimText`.
    #[allow(clippy::too_many_arguments)]
    pub fn big(
        &mut self,
        text: &str,
        secondary: &str,
        delay: f64,
        do_scale: bool,
        color: &str,
        secondary_color: &str,
        removable: bool,
        size_mult: f64,
        screen: Screen,
        t: &mut Text,
        restore: &mut dyn FnMut(),
    ) {
        if !self.deactivate_big() {
            return;
        }
        if !text.is_empty() {
            self.start(
                text,
                screen.max_w / 2.0,
                BIG_TEXT_Y,
                -0.1,
                BIG_TEXT_SIZE * size_mult,
                if do_scale { 0.005 } else { 0.0 },
                false,
                delay,
                delay,
                removable,
                color,
                true,
                8,
                true,
                screen,
                t,
                restore,
            );
        }
        if !secondary.is_empty() {
            self.start(
                secondary,
                screen.max_w / 2.0,
                BIG_TEXT_Y + TEXT_GAP * screen.view_mult * size_mult,
                -0.04,
                (MED_TEXT_SIZE / 2.0) * size_mult,
                if do_scale { 0.003 } else { 0.0 },
                false,
                delay,
                delay,
                removable,
                secondary_color,
                true,
                8,
                true,
                screen,
                t,
                restore,
            );
        }
    }

    /// KRP `startMovingAnimText` (damage and heal numbers).
    #[allow(clippy::too_many_arguments)]
    pub fn moving(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        color: &str,
        extra_size: f64,
        screen: Screen,
        t: &mut Text,
        restore: &mut dyn FnMut(),
    ) {
        let x = x + random_int(-25, 25) as f64;
        let y = y + random_int(-20, 5) as f64;
        self.start(
            text,
            x,
            y,
            -0.15,
            screen.max_h / 26.0 + extra_size,
            0.005,
            true,
            MOVING_TEXT_FADE_DELAY,
            0.0,
            false,
            color,
            false,
            5,
            false,
            screen,
            t,
            restore,
        );
    }
}

impl Default for AnimTexts {
    fn default() -> Self {
        Self::new()
    }
}

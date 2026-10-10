//! KRP's start menu (`#startMenuWrapper`): room browser, name and
//! ENTER GAME, and the loadout's class picker.
//!
//! Laid out in KRP's CSS pixels inside the 1050 px wrapper, which KRP
//! centres at 45% height and scales by `uiScale`. Account, mods,
//! cosmetics, settings and custom rooms are not ported yet.

#![forbid(unsafe_code)]

use macroquad::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use super::Game;
use super::hud::{BASE, LINE, Ui};
use crate::gfx::hex;
use crate::platform::Fetch;

/// One row of `/api/getRooms`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RoomInfo {
    pub n: String,
    pub m: String,
    pub lb: Value,
    pub pl: Value,
    pub mxpl: Value,
}

#[derive(Default)]
pub struct Menu {
    pub rooms: Vec<RoomInfo>,
    rooms_fetch: Option<Fetch>,
    /// The room the list was last fetched for, and whether it was fetched.
    rooms_for: Option<String>,
    rooms_fresh: bool,
    pub class_screen: bool,
    pub tab: usize,
    /// The wrapper's height from the last layout pass.
    pub height: f32,
    pub join: Option<String>,
}

const WRAP_W: f32 = 1050.0;
const GRAY: &str = "#2e3031";
const BLUE: &str = "#76b3e3";
const BLUE_DARK: &str = "#6fa9d6";

fn js(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n
            .as_f64()
            .map_or_else(|| n.to_string(), super::events::fmt_num),
        _ => String::new(),
    }
}

/// A KRP menu button (`#startButton` and friends).
fn button(ui: &mut Ui, label: &str, x: f32, y: f32, w: f32, h: f32, size: f32) -> bool {
    let (hover, click) = ui.hit(x, y, w, h);
    let (y, bg, size) = if hover {
        (y + 1.0, hex(BLUE_DARK), size + 2.0)
    } else {
        (y, hex(BLUE), size)
    };
    ui.rect(x, y, w, h, bg);
    if !hover {
        ui.rect(x, y + h - 3.0, w, 3.0, hex(BLUE_DARK));
    }
    ui.label_c(label, x + w / 2.0, y + (h - size * LINE) / 2.0, size, WHITE);
    click
}

fn panel(ui: &Ui, x: f32, y: f32, w: f32, h: f32) {
    ui.rect(x, y, w, h, WHITE);
    ui.rect(x, y + h - 5.0, w, 5.0, hex("#e0e0e0"));
}

impl Game {
    /// Keeps the room list fresh, as KRP's `RoomList` does when the room
    /// changes or REFRESH is clicked.
    pub fn poll_menu(&mut self) {
        let stale = !self.menu.rooms_fresh || self.menu.rooms_for != self.room;
        if stale && self.menu.rooms_fetch.is_none() {
            self.menu.rooms_for.clone_from(&self.room);
            self.menu.rooms_fresh = true;
            self.menu.rooms_fetch = Some(Fetch::start(&format!("{}/api/getRooms", self.base)));
        }
        if let Some(f) = &mut self.menu.rooms_fetch {
            if let Some(r) = f.poll() {
                self.menu.rooms_fetch = None;
                if let Ok(b) = r {
                    if let Ok(rooms) = serde_json::from_slice::<Vec<RoomInfo>>(&b) {
                        self.menu.rooms = rooms;
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn can_start(&self) -> bool {
        self.room.is_some() && !self.changing_lobby
    }

    /// Lays out (and, unless `ui.dry`, draws) the start menu in menu units.
    #[allow(clippy::too_many_lines)]
    pub fn draw_start_menu(&mut self, ui: &mut Ui) {
        // #mainTitleText: 96px with six 1px #e0e0e0 shadow steps.
        let title = "VERTIX ONLINE";
        for i in (1..=6).rev() {
            ui.label_c(title, WRAP_W / 2.0, i as f32, 96.0, hex("#e0e0e0"));
        }
        ui.label_c(title, WRAP_W / 2.0, 0.0, 96.0, WHITE);
        let top = 96.0 * LINE;
        let gray = hex(GRAY);
        let black = BLACK;

        // #roomWrapper
        let rw = WRAP_W * 0.355;
        let mut y = top + 20.0;
        let x = 20.0;
        let inner = rw - 40.0;
        let header_h = 16.0 * LINE;
        let rb_w = ui.measure("REFRESH", 12.0) + 12.0;
        let rb_h = 12.0 * LINE + 12.0;
        let room_rows = self.menu.rooms.len().min(16) as f32;
        let row_h = 12.0 * LINE + 10.0;
        let list_h = (room_rows * row_h).clamp(14.0 * LINE, 265.0);
        let room_h = 20.0 + header_h.max(rb_h) + 10.0 + list_h + 20.0;
        panel(ui, 0.0, top, rw, room_h);
        ui.label(
            "ROOM BROWSER",
            x,
            y + (rb_h.max(header_h) - header_h) / 2.0,
            16.0,
            gray,
        );
        // KRP's header is 125% wide, so the button sits past the panel edge.
        let rbx = x + inner * 1.25 - rb_w;
        if button(ui, "REFRESH", rbx, y, rb_w, rb_h, 12.0) {
            self.menu.rooms_fresh = false;
        }
        y += header_h.max(rb_h) + 10.0;
        if self.menu.rooms.is_empty() {
            ui.label("Loading...", x, y, 12.0, black);
        }
        let mut join = None;
        for r in self.menu.rooms.iter().take(16) {
            if y + row_h > top + room_h - 20.0 {
                break;
            }
            let selected = self.room.as_deref() == Some(r.n.as_str());
            let (hover, click) = ui.hit(x, y, inner, row_h);
            let size = if hover || selected { 14.0 } else { 12.0 };
            if hover || selected {
                ui.rect(x, y, inner, row_h, Color::new(0.0, 0.0, 0.0, 0.1));
            }
            let ty = y + (row_h - size * LINE) / 2.0;
            ui.label(&format!("{}_{}", r.m, r.n), x + 5.0, ty, size, black);
            let right = format!("{}% - {}/{}", js(&r.lb), js(&r.pl), js(&r.mxpl));
            let rwid = ui.measure(&right, size);
            ui.label(&right, x + inner - 5.0 - rwid, ty, size, black);
            if click {
                join = Some(r.n.clone());
            }
            y += row_h;
        }
        if let Some(j) = join {
            self.menu.join = Some(j);
        }

        // #startMenu
        let sx0 = rw + 10.0;
        let sw = WRAP_W * 0.305;
        let sx = sx0 + 20.0;
        let si = sw - 40.0;
        let mut y = top + 20.0;
        let start_h = 20.0
            + 16.0 * LINE
            + 10.0
            + (14.0 * LINE + 22.0)
            + 10.0
            + 4.0 * 61.0
            + 12.0 * LINE
            + 25.0;
        panel(ui, sx0, top, sw, start_h);
        ui.label_c("MAIN MENU", sx0 + sw / 2.0, y, 16.0, gray);
        y += 16.0 * LINE + 10.0;
        let ih = 14.0 * LINE + 22.0;
        ui.rect(sx, y, si, ih, hex("#dcdcdc"));
        ui.rect(sx + 1.0, y + 1.0, si - 2.0, ih - 2.0, WHITE);
        let shown = if self.player_name.is_empty() {
            ("Player Name".to_owned(), hex("#a9a9a9"))
        } else {
            let caret = if (self.current_time / 500.0) as i64 % 2 == 0 {
                "|"
            } else {
                ""
            };
            (format!("{}{caret}", self.player_name), black)
        };
        ui.label_c(&shown.0, sx + si / 2.0, y + 11.0, 14.0, shown.1);
        y += ih + 10.0;
        let can_start = self.can_start();
        if button(ui, "ENTER GAME", sx, y + 10.0, si, 41.0, 15.0) && can_start {
            self.start_game();
        }
        y += 61.0;
        button(ui, "LEADERBOARDS", sx, y + 10.0, si, 41.0, 15.0);
        y += 61.0;
        button(ui, "SETTINGS", sx, y + 10.0, si, 41.0, 15.0);
        y += 61.0;
        button(ui, "CONTROLS", sx, y + 10.0, si, 41.0, 15.0);
        y += 61.0;
        ui.label_c("Thanks for playing", sx0 + sw / 2.0, y, 12.0, black);

        // #rightMenu
        let rw2 = WRAP_W * 0.32;
        let rx0 = WRAP_W - rw2;
        let rx = rx0 + 20.0;
        let ri = rw2 - 40.0;
        let mut y = top + 20.0;
        let soft = Color::new(0.0, 0.0, 0.0, 0.4);
        let mut right_h = 20.0 + 16.0 * LINE + 22.0 + 12.0;
        let class_rows = self.classes.iter().filter(|c| c.name != "???").count() as f32;
        right_h += if self.menu.class_screen {
            16.0 * LINE + 10.0 + class_rows * (12.0 * LINE + 10.0)
        } else {
            16.0 * LINE + 3.0 + 3.0 * (12.0 * LINE + 10.0)
        } + 20.0;
        panel(ui, rx0, top, rw2, right_h);
        let mut tx = rx;
        for (i, t) in ["SETUP", "ACCOUNT", "MODS"].iter().enumerate() {
            let tw = ui.measure(t, 16.0) + 22.0;
            let th = 16.0 * LINE + 22.0;
            let (hover, click) = ui.hit(tx, y, tw, th);
            let bg = if self.menu.tab == i {
                "#d9d9d9"
            } else if hover {
                "#e6e6e6"
            } else {
                "#f2f2f2"
            };
            ui.rect(tx, y, tw, th, hex(bg));
            ui.label(t, tx + 11.0, y + 11.0, 16.0, black);
            if click {
                self.menu.tab = i;
            }
            tx += tw;
        }
        y += 16.0 * LINE + 22.0 + 12.0;
        if self.menu.tab != 0 {
            ui.label("Not available in this client yet.", rx, y, 12.0, soft);
        } else if self.menu.class_screen {
            ui.label("SELECT CLASS", rx, y, 16.0, gray);
            y += 16.0 * LINE + 10.0;
            let mut pick = None;
            for (i, c) in self.classes.iter().enumerate() {
                if c.name == "???" {
                    continue;
                }
                let h = 12.0 * LINE + 10.0;
                let (hover, click) = ui.hit(rx, y, ri, h);
                let size = if hover { 14.0 } else { 12.0 };
                if hover {
                    ui.rect(rx, y, ri, h, Color::new(0.0, 0.0, 0.0, 0.1));
                }
                ui.label(&c.name, rx + 5.0, y + (h - size * LINE) / 2.0, size, soft);
                if click {
                    pick = Some(i);
                }
                y += h;
            }
            if let Some(i) = pick {
                self.loadout_class = i;
                self.menu.class_screen = false;
            }
        } else {
            ui.label("LOADOUT", rx, y, 16.0, gray);
            y += 16.0 * LINE + 3.0;
            let class = self.classes.get(self.loadout_class).cloned();
            let rows = [
                (
                    "Class: ",
                    class.as_ref().map_or(String::new(), |c| c.name.clone()),
                    true,
                ),
                (
                    "Primary: ",
                    class.as_ref().map_or(String::new(), |c| c.primary.clone()),
                    false,
                ),
                (
                    "Secondary: ",
                    class
                        .as_ref()
                        .map_or(String::new(), |c| c.secondary.clone()),
                    false,
                ),
            ];
            for (label, value, clickable) in rows {
                let h = 12.0 * LINE + 10.0;
                ui.label(label, rx, y + 5.0, 12.0, soft);
                let lw = ui.measure(label, 12.0);
                let vw = ui.measure(&value, 14.0) + 10.0;
                let (hover, click) = if clickable {
                    ui.hit(rx + lw, y, vw, h)
                } else {
                    (false, false)
                };
                let size = if hover { 14.0 } else { 12.0 };
                if hover {
                    ui.rect(rx + lw, y, vw, h, Color::new(0.0, 0.0, 0.0, 0.1));
                }
                ui.label(
                    &value,
                    rx + lw + 5.0,
                    y + 5.0 + (12.0 - size) * BASE,
                    size,
                    soft,
                );
                if click {
                    self.menu.class_screen = true;
                }
                y += h - 5.0;
            }
        }
        self.menu.height = top + room_h.max(start_h).max(right_h) + 10.0;
    }

    /// The wrapper's transform: menu units to CSS pixels.
    #[must_use]
    pub fn menu_transform(&self, css: (f32, f32)) -> (f32, Vec2) {
        let (w, h) = css;
        let scale = ((h + w) / (1920.0 + 1080.0)) * 1.25;
        let origin = vec2(
            w / 2.0 - WRAP_W / 2.0 * scale,
            h * 0.45 - self.menu.height / 2.0 * scale,
        );
        (scale, origin)
    }
}

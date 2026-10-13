//! KRP's HTML overlay (the Svelte components and `main.css`), drawn in the
//! engine: minimap, team progress, ping and FPS, leaderboard, score,
//! health and ammo, the weapon bar, chat, and the end-of-round table.
//!
//! Positions and sizes are KRP's CSS values in CSS pixels. Text is drawn
//! at the screen's pixel density so it stays as sharp as the browser's.

#![forbid(unsafe_code)]

use macroquad::prelude::*;
use serde_json::json;

use super::draw::MINIMAP_SIZE;
use super::events::fmt_num;
use super::{Game, TEAM_BLUE, TEAM_RED};
use crate::gfx::{Image, hex};
use crate::text::Text;

/// CSS `normal` line height and baseline for `font2.ttf` (hhea ascent
/// 1.125, descent 0.25, no line gap).
pub const LINE: f32 = 1.375;
pub const BASE: f32 = 1.125;

/// Immediate-mode drawing and hit testing in CSS pixels (or menu units).
pub struct Ui<'a> {
    pub text: &'a Text,
    /// Device pixels per unit, for sharp text.
    pub px: f32,
    pub mouse: Vec2,
    pub clicked: bool,
    /// Set when the mouse is over something clickable.
    pub hot: bool,
    /// Layout only: nothing is drawn and nothing is clicked.
    pub dry: bool,
}

impl Ui<'_> {
    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        if !self.dry && w > 0.0 && h > 0.0 {
            draw_rectangle(x, y, w, h, c);
        }
    }

    #[must_use]
    pub fn measure(&self, s: &str, size: f32) -> f32 {
        self.text.measure_px(s, size, self.px)
    }

    /// Text with its line box's top at `y`.
    pub fn label(&self, s: &str, x: f32, y: f32, size: f32, c: Color) {
        if !self.dry && !s.is_empty() {
            self.text.draw_px(s, x, y + size * BASE, size, self.px, c);
        }
    }

    /// Text centred horizontally on `cx`.
    pub fn label_c(&self, s: &str, cx: f32, y: f32, size: f32, c: Color) {
        let w = self.measure(s, size);
        self.label(s, cx - w / 2.0, y, size, c);
    }

    /// `(hovered, clicked)` for a box.
    pub fn hit(&mut self, x: f32, y: f32, w: f32, h: f32) -> (bool, bool) {
        if self.dry {
            return (false, false);
        }
        let over =
            self.mouse.x >= x && self.mouse.x < x + w && self.mouse.y >= y && self.mouse.y < y + h;
        if over {
            self.hot = true;
        }
        (over, over && self.clicked)
    }

    pub fn image(&self, img: &Image, x: f32, y: f32, w: f32, h: f32) {
        if !self.dry {
            draw_texture_ex(
                &img.tex,
                x,
                y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(w, h)),
                    flip_x: img.flip_x,
                    flip_y: img.flip_y,
                    ..Default::default()
                },
            );
        }
    }
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba(r, g, b, (a * 255.0).round() as u8)
}

fn dim() -> Color {
    rgba(0, 0, 0, 0.1)
}

/// A padded box with a background, sized to its text (`.gameDevStat`,
/// `#score`, `#health`, `#ammo`).
fn stat_box(ui: &Ui, parts: &[(&str, f32, Color)], x: f32, y: f32, right: bool) -> (f32, f32) {
    let w: f32 = parts
        .iter()
        .map(|(s, size, _)| ui.measure(s, *size))
        .sum::<f32>()
        + 20.0;
    let size = parts.iter().map(|p| p.1).fold(0.0, f32::max);
    let h = size * LINE + 20.0;
    let x = if right { x - w } else { x };
    ui.rect(x, y, w, h, dim());
    let mut cx = x + 10.0;
    for (s, sz, c) in parts {
        // Mixed sizes share a baseline.
        ui.label(s, cx, y + 10.0 + (size - sz) * BASE, *sz, *c);
        cx += ui.measure(s, *sz);
    }
    (w, h)
}

impl Game {
    /// Whether KRP's in-game overlay (`showUI`) is up.
    #[must_use]
    pub fn hud_visible(&self) -> bool {
        self.game_start
            && !self.in_main_menu
            && !self.stat_table
            && !self.game_over
            && self.settings.show_ui
    }

    /// Draws the overlay in CSS pixels.
    #[allow(clippy::too_many_lines)]
    pub fn draw_hud(&mut self, minimap: Option<&Image>, ui: &mut Ui, css: (f32, f32)) {
        let (w, h) = css;
        let white = WHITE;
        let soft = rgba(255, 255, 255, 0.7);
        if self.hud_visible() {
            // #statContainer2: minimap, team progress, mode text.
            ui.rect(10.0, 10.0, 200.0, 200.0, rgba(0, 0, 0, 0.2));
            if let Some(m) = minimap {
                ui.image(m, 10.0, 10.0, MINIMAP_SIZE, MINIMAP_SIZE);
            }
            let ty = 220.0;
            ui.rect(10.0, ty, 200.0, 55.0, rgba(0, 0, 0, 0.2));
            let teams = self.map.as_ref().is_some_and(|m| m.mode.teams);
            let (blue_pct, red_pct, label) = self.progress;
            let rows: Vec<(&str, Color, f64)> = if teams {
                vec![
                    ("A", hex(TEAM_BLUE), blue_pct),
                    ("B", hex(TEAM_RED), red_pct),
                ]
            } else {
                vec![(label, hex(TEAM_BLUE), blue_pct)]
            };
            let row_h = (55.0 - 5.0 * (rows.len() as f32 + 1.0)) / rows.len() as f32;
            let label_w = rows
                .iter()
                .map(|r| ui.measure(r.0, 14.0))
                .fold(0.0, f32::max);
            for (i, (t, c, pct)) in rows.iter().enumerate() {
                let ry = ty + 5.0 + i as f32 * (row_h + 5.0);
                ui.label(t, 20.0, ry + (row_h - 14.0 * LINE) / 2.0, 14.0, *c);
                let bx = 20.0 + label_w + 10.0;
                let bw = 200.0 - (bx - 10.0) - 10.0;
                let by = ry + (row_h - 12.0) / 2.0;
                ui.rect(bx, by, bw, 12.0, dim());
                ui.rect(bx, by, bw * (*pct as f32 / 100.0).clamp(0.0, 1.0), 12.0, *c);
            }
            if !self.mode_text.is_empty() {
                ui.label(
                    &self.mode_text,
                    20.0,
                    ty + 55.0 + 20.0,
                    14.0,
                    hex("#ffd100"),
                );
            }

            // #conStatContainer
            if self.settings.show_ping_fps {
                let (_, bh) = stat_box(
                    ui,
                    &[(&format!("PING {}", fmt_num(self.ping)), 12.0, white)],
                    220.0,
                    10.0,
                    false,
                );
                stat_box(
                    ui,
                    &[(&format!("FPS {}", self.fps.round()), 12.0, white)],
                    220.0,
                    20.0 + bh,
                    false,
                );
            }

            // #statContainer3: leaderboard and score.
            let mut lb_bottom = 10.0;
            if self.settings.show_leader {
                let mut lines: Vec<(String, Color)> = Vec::new();
                let my_team = self.me().team.clone();
                for (i, idx) in self.leaderboard.iter().enumerate() {
                    let Some(p) = self.find(*idx) else { continue };
                    let clan = if p.account.clan.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", p.account.clan)
                    };
                    if Some(p.index) == self.me {
                        lines.push((format!("{}. {}{clan}", i + 1, self.player_name), white));
                    } else if !p.name.is_empty() {
                        let c = if p.team == my_team {
                            TEAM_BLUE
                        } else {
                            TEAM_RED
                        };
                        lines.push((format!("{}. {}{clan}", i + 1, p.name), hex(c)));
                    }
                }
                let title_w = ui.measure("LEADERBOARD", 23.0);
                let inner = lines
                    .iter()
                    .map(|(s, _)| ui.measure(s, 15.0))
                    .fold(title_w, f32::max)
                    .min(330.0);
                let bw = inner + 20.0;
                let bh = 23.0 * LINE + lines.len() as f32 * 15.0 * LINE + 20.0;
                let bx = w - 10.0 - bw;
                ui.rect(bx, 10.0, bw, bh, dim());
                ui.label_c("LEADERBOARD", bx + bw / 2.0, 20.0, 23.0, white);
                for (i, (s, c)) in lines.iter().enumerate() {
                    ui.label_c(
                        s,
                        bx + bw / 2.0,
                        20.0 + 23.0 * LINE + i as f32 * 15.0 * LINE,
                        15.0,
                        *c,
                    );
                }
                lb_bottom = 10.0 + bh;
            }
            let score = fmt_num(self.me().score);
            stat_box(
                ui,
                &[("SCORE ", 18.0, soft), (&score, 18.0, white)],
                w - 10.0,
                lb_bottom + 10.0,
                true,
            );

            // #statContainer: ammo and health, bottom right.
            let me = self.me().clone();
            let health_c = if me.health <= 10.0 {
                hex("#e06363")
            } else {
                white
            };
            let ammo = me
                .weapon()
                .map_or_else(|| "0".to_owned(), |wp| fmt_num(wp.ammo));
            let box_h = 18.0 * LINE + 20.0;
            let by = h - 15.0 - box_h;
            let (hw, _) = stat_box(
                ui,
                &[
                    ("HEALTH ", 18.0, soft),
                    (&fmt_num(me.health), 18.0, health_c),
                ],
                w - 10.0,
                by,
                true,
            );
            stat_box(
                ui,
                &[("AMMO ", 18.0, soft), (&ammo, 18.0, white)],
                w - 10.0 - hw - 10.0,
                by,
                true,
            );

            // #actionBar: weapon icons with reload sweep.
            let mut x = w - 10.0;
            for (slot, wp) in me.weapons.iter().enumerate().rev() {
                let size = if slot == me.current_weapon {
                    55.0
                } else {
                    45.0
                };
                x -= 10.0 + size;
                let y = h - 74.0 - size;
                let icon = self
                    .sprites
                    .weapons
                    .get(wp.spec.weapon_index)
                    .and_then(|s| s.icon.clone());
                if let Some(icon) = icon {
                    if let Some((start, len)) = self.cooldowns.get(slot).copied() {
                        if len > 0.0 {
                            let t = ((self.current_time - start) / len).clamp(0.0, 1.0) as f32;
                            if t < 1.0 {
                                let ch = size * (1.0 - t);
                                ui.rect(x, y + size - ch, size, ch, rgba(255, 255, 255, 0.2));
                            }
                        }
                    }
                    ui.rect(x, y, size, size, dim());
                    ui.image(&icon, x, y, size, size);
                }
            }
        }

        if self.chat_visible && self.settings.show_chat {
            self.draw_chat(ui, h);
        }
        if self.stat_table {
            self.draw_stat_table(ui, w, h);
        }
    }

    /// `#chatbox`.
    fn draw_chat(&mut self, ui: &mut Ui, h: f32) {
        let input_h = 14.0 * LINE + 16.0;
        let input_y = h - 15.0 - input_h;
        let list_bottom = input_y;
        let list_top = list_bottom - 276.0;
        let mut y = list_bottom - 8.0;
        for line in self.chat.iter().rev() {
            let lh = 16.0 * LINE + 4.0;
            y -= lh + 6.0;
            if y < list_top {
                break;
            }
            let (who, wc) = match line.source.as_str() {
                "system" => (String::new(), hex("#db4fcd")),
                "notif" => (String::new(), WHITE),
                "me" => ("YOU: ".to_owned(), WHITE),
                "blue" => (format!("{}: ", line.author), hex(TEAM_BLUE)),
                _ => (format!("{}: ", line.author), hex(TEAM_RED)),
            };
            let x = 10.0 + 8.0 + 5.0;
            if who.is_empty() {
                ui.label(&line.text, x, y + 2.0, 16.0, wc);
            } else {
                ui.label(&who, x, y + 2.0, 16.0, wc);
                let ww = ui.measure(&who, 16.0);
                ui.label(&line.text, x + ww, y + 2.0, 16.0, rgba(255, 255, 255, 0.7));
            }
        }
        ui.rect(10.0, input_y, 266.0, input_h, rgba(0, 0, 0, 0.2));
        match &self.chat_input {
            Some(t) => {
                let caret = if (self.current_time / 500.0) as i64 % 2 == 0 {
                    "|"
                } else {
                    ""
                };
                ui.label(&format!("{t}{caret}"), 18.0, input_y + 8.0, 14.0, WHITE);
            }
            None => ui.label(
                "Send Message...",
                18.0,
                input_y + 8.0,
                14.0,
                rgba(255, 255, 255, 0.5),
            ),
        }
        let tw = ui.measure(&self.chat_team_text(), 14.0) + 16.0;
        ui.rect(10.0 + 266.0 + 5.0, input_y, tw, input_h, rgba(0, 0, 0, 0.2));
        ui.label(
            &self.chat_team_text(),
            10.0 + 266.0 + 13.0,
            input_y + 8.0,
            14.0,
            WHITE,
        );
        if ui.hit(281.0, input_y, tw, input_h).1 {
            self.chat_team = !self.chat_team;
        }
    }

    fn chat_team_text(&self) -> String {
        if self.chat_team {
            "TEAM".into()
        } else {
            "ALL".into()
        }
    }

    /// Sends the typed chat line (KRP `sendChat`).
    pub fn send_chat(&mut self) {
        let Some(t) = self.chat_input.take() else {
            return;
        };
        if t.is_empty() {
            return;
        }
        let t: String = t.chars().take(50).collect();
        let kind = self.chat_team_text();
        self.emit("cht", vec![json!(t), json!(kind)]);
        let prefix = if self.chat_team { "(TEAM) " } else { "" };
        let name = self.player_name.clone();
        self.add_chat_line(&name, &format!("{prefix}{t}"), "me");
    }

    /// `#gameStatWrapper`: round timer, result, score table and votes.
    #[allow(clippy::too_many_lines)]
    fn draw_stat_table(&mut self, ui: &mut Ui, w: f32, h: f32) {
        let zone = self.map.as_ref().is_some_and(|m| m.mode.code == "zmtch");
        let mut players: Vec<_> = self
            .players
            .iter()
            .filter(|p| !p.team.is_empty())
            .cloned()
            .collect();
        players.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.index.cmp(&a.index))
        });
        let heads = [
            "NAME",
            "SCORE",
            "KILLS",
            "DEATHS",
            "DAMAGE",
            if zone { "GOALS" } else { "HEALING" },
            "REWARD",
        ];
        let my_team = self.me().team.clone();
        let rows: Vec<(Vec<String>, Color, u32, usize)> = players
            .iter()
            .map(|p| {
                let c = if Some(p.index) == self.me {
                    WHITE
                } else if p.team == my_team {
                    hex(TEAM_BLUE)
                } else {
                    hex(TEAM_RED)
                };
                let likes = p.liked_by.as_array().map_or(0, Vec::len);
                (
                    vec![
                        p.name.clone(),
                        fmt_num(p.score),
                        fmt_num(p.kills),
                        fmt_num(p.deaths),
                        fmt_num(p.total_damage),
                        fmt_num(if zone { p.total_goals } else { p.total_healing }),
                        "No Reward".into(),
                    ],
                    c,
                    p.index,
                    likes,
                )
            })
            .collect();
        // Column widths from content (border-spacing 22px).
        let mut cols: Vec<f32> = heads.iter().map(|s| ui.measure(s, 17.0)).collect();
        for (cells, ..) in &rows {
            for (i, c) in cells.iter().enumerate() {
                let size = if i == 0 { 13.0 } else { 19.0 };
                cols[i] = cols[i].max(ui.measure(c, size).min(250.0));
            }
        }
        let nice_w = ui.measure("NICE", 12.0) + 16.0;
        let like_w = ui.measure("00", 19.0);
        let table_w: f32 = cols.iter().sum::<f32>() + nice_w + like_w + 22.0 * 10.0;
        let row_h = 19.0 * LINE + 5.0;
        let result = self.winner_text.clone();
        let box_w = table_w + 40.0;
        let box_h = 40.0 + 20.0 * LINE + 4.0 + 17.0 * LINE + 10.0 + rows.len() as f32 * row_h;
        let timer_h = 18.0 * LINE + 36.0;
        let votes_h = if self.votes.is_empty() {
            0.0
        } else {
            36.0 + 24.0
        };
        let total_h = timer_h + box_h + 10.0 + votes_h;
        let top = h * 0.45 - total_h / 2.0;
        let cx = w / 2.0;
        let timer = if self.next_game_text.is_empty() {
            "GAME STATS".to_owned()
        } else {
            self.next_game_text.clone()
        };
        ui.label_c(&timer, cx, top + 18.0, 18.0, WHITE);
        let bx = cx - box_w / 2.0;
        let by = top + timer_h;
        ui.rect(bx, by, box_w, box_h, rgba(0, 0, 0, 0.2));
        let mut y = by + 20.0;
        if let Some((t, c)) = &result {
            ui.label_c(t, cx, y + 2.0, 20.0, hex(c));
        }
        y += 20.0 * LINE + 4.0;
        let mut x = bx + 20.0 + 22.0;
        for (i, head) in heads.iter().enumerate() {
            if i == 0 {
                ui.label(head, x, y, 17.0, WHITE);
            } else {
                ui.label_c(head, x + cols[i] / 2.0, y, 17.0, WHITE);
            }
            x += cols[i] + 22.0;
        }
        y += 17.0 * LINE + 10.0;
        let my_index = self.me;
        let mut like_click = None;
        for (cells, color, index, likes) in &rows {
            let mut x = bx + 20.0 + 22.0;
            for (i, c) in cells.iter().enumerate() {
                if i == 0 {
                    ui.label(c, x, y + (19.0 - 13.0) * BASE, 13.0, *color);
                } else {
                    ui.label_c(c, x + cols[i] / 2.0, y, 19.0, WHITE);
                }
                x += cols[i] + 22.0;
            }
            if Some(*index) != my_index {
                let active = self.current_liked == Some(*index);
                let bg = if active {
                    rgba(255, 255, 255, 0.2)
                } else {
                    rgba(0, 0, 0, 0.15)
                };
                ui.rect(x, y, nice_w, 12.0 * LINE + 8.0, bg);
                ui.label("NICE", x + 8.0, y + 4.0, 12.0, WHITE);
                if ui.hit(x, y, nice_w, 12.0 * LINE + 8.0).1 {
                    like_click = Some(*index);
                }
            }
            x += nice_w + 22.0;
            ui.label(&likes.to_string(), x, y, 19.0, WHITE);
            y += row_h;
        }
        if let Some(i) = like_click {
            self.like(i);
        }
        // #voteModeContainer
        if !self.votes.is_empty() {
            let vy = by + box_h + 10.0 + 8.0;
            let vw = box_w.max(400.0) * 0.28;
            let n = self.votes.len() as f32;
            let row_w = n * (vw + 16.0);
            let mut vx = cx - row_w / 2.0 + 8.0;
            let mut vote = None;
            for (i, (name, count)) in self.votes.iter().enumerate() {
                let active = self.my_vote == Some(i);
                let (hover, click) = ui.hit(vx, vy, vw, 36.0);
                let bg = match (active, hover) {
                    (_, true) => rgba(255, 255, 255, 0.1),
                    (true, false) => rgba(255, 255, 255, 0.2),
                    (false, false) => rgba(0, 0, 0, 0.15),
                };
                ui.rect(vx, vy, vw, 36.0, bg);
                ui.label_c(
                    &format!("{name}: {}", fmt_num(*count)),
                    vx + vw / 2.0,
                    vy + (36.0 - 14.0 * LINE) / 2.0,
                    14.0,
                    WHITE,
                );
                if click {
                    vote = Some(i);
                }
                vx += vw + 16.0;
            }
            if let Some(i) = vote {
                self.emit("modeVote", vec![json!(i)]);
                self.my_vote = if self.my_vote == Some(i) {
                    None
                } else {
                    Some(i)
                };
            }
        }
    }

    /// KRP `onClickNice`.
    fn like(&mut self, dest: u32) {
        let me = self.me.unwrap_or(0);
        self.emit("like", vec![json!(me), json!(dest)]);
        if self.current_liked == Some(dest) {
            self.current_liked = None;
        } else {
            if let Some(prev) = self.current_liked {
                self.emit("like", vec![json!(me), json!(prev)]);
            }
            self.current_liked = Some(dest);
        }
    }
}

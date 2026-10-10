//! KRP's `setupSocket` handlers: what each server event does.

#![forbid(unsafe_code)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use vertix_sim::projectile::Shot;

use super::model::{Player, num};
use super::rand::random_int;
use super::{Bullet, ChatLine, Game, Gfx, SprayState, TEAM_BLUE, TEAM_RED, TimerKind};
use crate::platform::now_ms;

fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::Null)
}

impl Game {
    #[allow(clippy::too_many_lines)]
    pub(super) fn on_event(&mut self, name: &str, args: &[Value], gfx: &mut Gfx) {
        match name {
            "pong1" => self.ping = (now_ms() - self.ping_start).round(),
            "yourRoom" => {
                self.room = arg(args, 0).as_str().map(str::to_owned);
                self.changing_lobby = false;
            }
            "error" => self.log.push(format!("server error: {}", arg(args, 0))),
            "welcome" => self.on_welcome(arg(args, 0), arg(args, 1).as_bool().unwrap_or(false)),
            "gameSetup" => {
                let setup: Value = arg(args, 0)
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(Value::Null);
                let map = arg(args, 1).as_bool().unwrap_or(false);
                let start = arg(args, 2).as_bool().unwrap_or(false);
                self.on_game_setup(&setup, map, start, gfx);
            }
            "lb" => {
                self.leaderboard = arg(args, 0)
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_u64().map(|n| n as u32))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            "ts" => self.on_team_scores(args),
            "rsd" => {
                self.updates_received += 1;
                self.on_server_data(arg(args, 0));
            }
            "upd" => self.on_update_user(arg(args, 0)),
            "vt" => {
                let v = arg(args, 0);
                let i = num(v.get("i")) as usize;
                if i < self.votes.len() {
                    self.votes[i] = (
                        v.get("n").and_then(Value::as_str).unwrap_or("").to_owned(),
                        num(v.get("v")),
                    );
                }
            }
            "add" => {
                let parsed = arg(args, 0)
                    .as_str()
                    .and_then(|s| serde_json::from_str::<Player>(s).ok());
                if let Some(p) = parsed {
                    if Some(p.index) != self.me {
                        match self.players.iter().position(|q| q.index == p.index) {
                            Some(i) => self.players[i] = p,
                            None => self.players.push(p),
                        }
                    }
                }
            }
            "crtSpr" => {
                let idx = num(args.first()) as u32;
                let (x, y) = (num(args.get(1)), num(args.get(2)));
                self.create_spray(idx, x, y);
            }
            "rem" => {
                let idx = num(args.first()) as u32;
                if Some(idx) != self.me {
                    self.players.retain(|p| p.index != idx);
                }
            }
            "cht" => {
                let a = match arg(args, 0) {
                    Value::Array(a) => a.clone(),
                    _ => args.to_vec(),
                };
                self.message_from_server(&a);
            }
            "kick" => {
                let r = arg(args, 0).as_str().unwrap_or("").to_owned();
                self.reason = Some(r.clone());
                self.kick(&r);
            }
            "1" => self.on_health(arg(args, 0), gfx),
            "2" => self.someone_shot(arg(args, 0)),
            "jum" => {
                let idx = num(args.first()) as u32;
                if Some(idx) != self.me {
                    if let Some(p) = self.find_mut(idx) {
                        super::player_jump(p);
                    }
                }
            }
            "ex" => {
                let me = self.me();
                let at = (me.x, me.y);
                self.fx
                    .explosion(num(args.first()), num(args.get(1)), num(args.get(2)), at);
            }
            "r" => {
                let wi = num(args.first()) as usize;
                let mut full = false;
                if let Some(p) = self.me.and_then(|m| self.find_mut(m).map(|_| m)) {
                    let plr = self.find_mut(p).expect("found");
                    if let Some(w) = plr.weapons.get_mut(wi) {
                        full = wi == 0 && w.max_ammo > 1.0;
                        w.reload_time = 0.0;
                        w.ammo = w.max_ammo;
                    }
                    if wi < self.cooldowns.len() {
                        self.cooldowns[wi] = (0.0, 0.0);
                    }
                }
                if full {
                    self.notify("Ammo Full", gfx);
                }
            }
            "3" => self.on_death(arg(args, 0), gfx),
            "4" => {
                let c = arg(args, 0);
                let index = num(args.get(1)) as usize;
                let kind = num(args.get(2));
                if let Some(m) = &mut self.map {
                    if kind == 0.0 {
                        if let (Some(p), Some(a)) = (
                            m.pickups.get_mut(index),
                            c.get("active").and_then(Value::as_bool),
                        ) {
                            p.active = a;
                        }
                    } else if let Some(clt) = m.world.clutter.get_mut(index) {
                        if let Some(a) = c.get("active").and_then(Value::as_bool) {
                            clt.active = a;
                        }
                        if let Some(x) = c.get("x").and_then(Value::as_f64) {
                            clt.x = x;
                        }
                        if let Some(y) = c.get("y").and_then(Value::as_f64) {
                            clt.y = y;
                        }
                    }
                }
            }
            "tprt" => self.on_teleport(arg(args, 0), gfx),
            "5" => {
                let t = arg(args, 0).as_str().unwrap_or("").to_owned();
                self.notify(&t, gfx);
            }
            "6" if !self.me().dead => {
                let t = arg(args, 0).as_str().unwrap_or("").to_owned();
                let s = arg(args, 1).as_str().unwrap_or("").to_owned();
                let m = num(args.get(2));
                self.big_text(gfx, &t, &s, 2000.0, true, "#ffffff", TEAM_BLUE, true, m);
            }
            "7" => {
                self.game_over = true;
                self.start_menu = false;
                let winner = arg(args, 0).clone();
                let votes = arg(args, 1).clone();
                let fading = arg(args, 2).as_bool().unwrap_or(false);
                self.show_stat_table(&votes, &winner, fading, gfx);
            }
            "8" => {
                self.next_game_text = format!("{}: UNTIL NEXT ROUND", value_text(arg(args, 0)));
            }
            _ => {}
        }
    }

    /// KRP `updateTeamScores`: the progress bars, as percentages of the
    /// mode's target score.
    fn on_team_scores(&mut self, args: &[Value]) {
        let Some(m) = &self.map else { return };
        let per = m.mode.score / 100.0;
        if m.mode.teams {
            let red = num(args.first()) / per;
            let blue = num(args.get(1)) / per;
            self.team_scores = Some((red, blue));
            self.progress = if self.me().team == "red" {
                (red, blue, "A")
            } else {
                (blue, red, "A")
            };
        } else {
            self.progress = (self.me().score / m.mode.score * 100.0, 0.0, "YOU");
        }
    }

    /// `welcome`: the server has a slot for us; answer with our name and
    /// class.
    fn on_welcome(&mut self, player: &Value, init: bool) {
        let mut p = player.clone();
        if let Some(o) = p.as_object_mut() {
            o.insert("name".into(), json!(self.player_name));
            o.insert("classIndex".into(), json!(self.loadout_class));
        }
        if let Some(r) = player.get("room").and_then(Value::as_str) {
            self.room = Some(r.to_owned());
        }
        self.emit(
            "gotit",
            vec![p, json!(init), json!(now_ms().floor()), json!(false)],
        );
        self.me_mut().dead = true;
        if init {
            self.anim.clear();
            self.game_start = false;
            self.start_menu = true;
        }
        if self.game_over {
            self.stat_table = false;
        }
        self.game_over = false;
        self.game_over_fade = false;
        self.target_changed = true;
    }

    fn on_game_setup(&mut self, setup: &Value, setup_map: bool, start: bool, gfx: &mut Gfx) {
        let you: Option<Player> = setup
            .get("you")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok());
        if setup_map {
            let map_data = setup.get("mapData").cloned().unwrap_or(Value::Null);
            let tile_scale = num(setup.get("tileScale"));
            self.players = setup
                .get("usersInRoom")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect()
                })
                .unwrap_or_default();
            self.map = Self::setup_map(&map_data, tile_scale);
            if let Some(m) = &self.map {
                let blue = you.as_ref().is_some_and(|y| y.team == "blue");
                self.mode_text = if blue {
                    m.mode.desc2.clone()
                } else {
                    m.mode.desc1.clone()
                };
            } else {
                self.log.push("gameSetup: could not read the map".into());
            }
            gfx.minimap_base = None;
            gfx.shadows.clear();
            for s in &mut self.sprays {
                s.active = false;
            }
            // KRP appends 100 more bullets on every new map.
            for i in 0..100 {
                let mut b = Bullet::default();
                b.p.server_index = i;
                self.bullets.push(b);
            }
        }
        if start {
            self.game_start = true;
            self.chat_visible = true;
        }
        self.keys.lm = false;
        let vm = num(setup.get("viewMult"));
        self.max_h = num(setup.get("maxScreenHeight")) * vm;
        self.max_w = num(setup.get("maxScreenWidth")) * vm;
        self.view_mult = vm;
        if let Some(you) = you {
            let idx = you.index;
            self.me = Some(idx);
            match self.players.iter().position(|p| p.index == idx) {
                Some(i) => self.players[i] = you,
                None => self.players.push(you),
            }
        }
        if self.in_main_menu {
            self.in_main_menu = false;
        }
        self.starting_game = false;
    }

    /// KRP `receiveServerData` (`rsd`): positions, then replaying our
    /// inputs the server has not applied yet.
    fn on_server_data(&mut self, data: &Value) {
        if !self.game_over {
            let d: Vec<f64> = data
                .as_array()
                .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
                .unwrap_or_default();
            for p in &mut self.players {
                p.on_screen = false;
            }
            let mut c = 0;
            while c < d.len() {
                let len = d[c] as usize;
                let idx = d.get(c + 1).copied().unwrap_or(-1.0);
                let at = |k: usize| d.get(c + k).copied().unwrap_or(0.0);
                let me = self.me;
                if let Some(p) = self.players.iter_mut().find(|p| f64::from(p.index) == idx) {
                    if Some(p.index) == me {
                        if len > 2 {
                            p.x = at(2);
                        }
                        if len > 3 {
                            p.y = at(3);
                        }
                        if len > 4 {
                            p.angle = at(4);
                        }
                        if len > 5 {
                            p.isn = at(5);
                        }
                    } else {
                        if len > 2 {
                            p.x_speed = (p.x - at(2)).abs();
                            p.x = at(2);
                        }
                        if len > 3 {
                            p.y_speed = (p.y - at(3)).abs();
                            p.y = at(3);
                        }
                        if len > 4 {
                            p.angle = at(4);
                        }
                        let front = super::is_weapon_facing_front(super::snap_angle(p.angle));
                        if let Some(w) = p.weapon_mut() {
                            w.front = front;
                        }
                        if len > 5 {
                            p.name_y_offset = at(5);
                        }
                    }
                    p.on_screen = true;
                } else if idx >= 0.0 {
                    self.emit("ftc", vec![json!(idx as u32)]);
                }
                if len == 0 {
                    break;
                }
                c += len;
            }
        }
        self.reconcile();
    }

    /// KRP `updateUserValue` (`upd`).
    fn on_update_user(&mut self, d: &Value) {
        let i = num(d.get("i")) as u32;
        let me = self.me;
        let Some(p) = self.find_mut(i) else {
            self.emit("ftc", vec![json!(i)]);
            return;
        };
        let f = |k: &str| d.get(k).and_then(Value::as_f64);
        if let Some(v) = f("s") {
            p.score = v;
        }
        if let Some(v) = d.get("sp").and_then(Value::as_bool) {
            p.is_spawn_protected = v;
        }
        if let Some(v) = f("wi") {
            if Some(i) != me {
                p.current_weapon = v as usize;
            }
        }
        if let Some(v) = d.get("l") {
            p.liked_by = v.clone();
        }
        if let Some(v) = f("dea") {
            p.deaths = v;
        }
        if let Some(v) = f("kil") {
            p.kills = v;
        }
        if let Some(v) = f("dmg") {
            p.total_damage = v;
        }
        if let Some(v) = f("hea") {
            p.total_healing = v;
        }
        if let Some(v) = f("goa") {
            p.total_goals = v;
        }
    }

    /// The `"1"` event: someone's health changed.
    fn on_health(&mut self, h: &Value, gfx: &mut Gfx) {
        let gid = num(h.get("gID")) as u32;
        let delta = num(h.get("healthDelta"));
        let me = self.me;
        if Some(gid) == me && delta < 0.0 {
            self.fx.shake.start(delta / 2.0, num(h.get("dir")));
        }
        let target = self.find(gid).cloned();
        let dealer = h.get("dID").and_then(Value::as_f64).map(|v| v as u32);
        if let Some(t) = &target {
            if dealer.is_some() && dealer == me && t.on_screen && delta < 0.0 {
                let dmg = delta.abs();
                self.moving_text(
                    gfx,
                    &fmt_num(dmg),
                    t.x - t.width / 2.0,
                    t.y - t.height,
                    TEAM_RED,
                    dmg / 10.0,
                );
            }
        }
        if let Some(bi) = h.get("bulletIndex").and_then(Value::as_f64) {
            let bi = bi as usize;
            if let Some(b) = self.bullets.iter_mut().find(|b| b.p.server_index == bi) {
                // A hit on a player this client cannot see has no position
                // to draw at; with persistent effects it is drawn where the
                // bullet is, so the blood is there when the spot comes into
                // view (KRP draws nothing).
                let off_screen_at = (self.opts.persistent_effects
                    && target.as_ref().is_none_or(|t| !t.on_screen))
                .then_some((b.p.x, b.p.y));
                if let Some((x, y)) = off_screen_at {
                    if delta < 0.0 && b.p.sprite_index != 2 {
                        let spread = PI / random_int(5, 7) as f64;
                        let dir = b.p.dir;
                        self.fx
                            .particle_cone(12, x, y, dir + PI, spread, 0.5, 16.0, 0, true);
                        self.fx.liquid(x, y, 4);
                    }
                }
                if Some(b.p.owner.index) != me {
                    if let Some(t) = &target {
                        if t.on_screen && delta < 0.0 && b.p.sprite_index != 2 {
                            let spread = PI / random_int(5, 7) as f64;
                            let dir = b.p.dir;
                            self.fx.particle_cone(
                                12,
                                t.x,
                                t.y - t.height / 2.0 - t.jump_y,
                                dir + PI,
                                spread,
                                0.5,
                                16.0,
                                0,
                                true,
                            );
                            self.fx.liquid(t.x, t.y, 4);
                        }
                    }
                    let b = self
                        .bullets
                        .iter_mut()
                        .find(|b| b.p.server_index == bi)
                        .expect("found above");
                    if b.p.pierce_count > 0 {
                        b.p.pierce_count -= 1;
                    }
                    if b.p.pierce_count == 0 {
                        b.p.active = false;
                    }
                }
            }
        }
        let health = num(h.get("health"));
        if let Some(p) = self.find_mut(gid) {
            p.health = health;
            if Some(gid) == me && delta > 0.0 {
                let (x, y, w, hh) = (p.x, p.y, p.width, p.height);
                self.moving_text(
                    gfx,
                    &fmt_num(delta),
                    x - w / 2.0,
                    y - hh,
                    "#5ed951",
                    delta / 10.0,
                );
            }
        }
    }

    /// KRP `someoneShot` (`"2"`).
    fn someone_shot(&mut self, e: &Value) {
        let i = num(e.get("i")) as u32;
        if Some(i) == self.me {
            return;
        }
        let si = num(e.get("si")) as usize;
        let Some(pi) = self.players.iter().position(|p| p.index == i) else {
            return;
        };
        let Some(bi) = self.bullets.iter().position(|b| b.p.server_index == si) else {
            return;
        };
        let shot = Shot {
            x: num(e.get("x")),
            y: num(e.get("y")),
            dir: num(e.get("d")),
            server_index: si,
        };
        self.arm_bullet(bi, pi, shot);
        if self.bullets[bi].p.active {
            self.bullets[bi].arrived = Some(self.event_at);
        }
    }

    /// The `"3"` event: someone died.
    #[allow(clippy::redundant_guards)] // float literals as patterns are deprecated
    fn on_death(&mut self, e: &Value, gfx: &mut Gfx) {
        let gid = num(e.get("gID")) as u32;
        let did = e.get("dID").and_then(Value::as_f64).map(|v| v as u32);
        let me = self.me;
        let dest_team = self.find(gid).map(|p| p.team.clone());
        let source = did.and_then(|d| self.find(d)).cloned();
        if let Some(p) = self.find_mut(gid) {
            p.dead = true;
        }
        let ss = value_text(e.get("sS").unwrap_or(&Value::Null));
        if e.get("kB").and_then(Value::as_bool).unwrap_or(false) && Some(gid) != me {
            if did.is_some() && did == me {
                self.big_text(
                    gfx,
                    "BOSS SLAIN",
                    &format!("{ss} POINTS"),
                    2000.0,
                    true,
                    "#ffffff",
                    TEAM_BLUE,
                    true,
                    1.25,
                );
            } else if let Some(s) = &source {
                let t = format!("{} slayed the boss", s.name);
                self.notify(&t, gfx);
            }
        } else if did.is_some() && did == me && Some(gid) != me {
            let same_team = dest_team.as_deref() == source.as_ref().map(|s| s.team.as_str());
            let (msg, points) = if same_team {
                ("Team Kill", "no".to_owned())
            } else {
                let kd = e.get("kd").and_then(Value::as_f64);
                let m = match kd {
                    None => "Enemy Killed",
                    Some(k) if k <= 1.0 => "Enemy Killed",
                    Some(k) if k == 2.0 => "Double Kill",
                    Some(k) if k == 3.0 => "Triple Kill",
                    Some(k) if k == 4.0 => "Multi Kill",
                    Some(k) if k == 5.0 => "Ultra Kill",
                    Some(k) if k == 6.0 => "No Way!",
                    Some(k) if k == 7.0 => "Stop!",
                    // Fractions fall through to here too, as in KRP's chain.
                    Some(_) => "Godlike!",
                };
                (m, format!("+{ss}"))
            };
            let msg = if e
                .get("ast")
                .is_some_and(|v| !v.is_null() && v != &json!(false) && v != &json!(0))
            {
                "Kill Assist"
            } else {
                msg
            };
            self.big_text(
                gfx,
                msg,
                &format!("{points} POINTS"),
                2000.0,
                true,
                "#ffffff",
                TEAM_BLUE,
                true,
                1.25,
            );
        }
        if Some(gid) == me {
            self.hide_stat_table(gfx);
            self.game_start = false;
            self.me_mut().dead = true;
            self.after(1300.0, TimerKind::ShowStartMenuAfterDeath);
        }
    }

    /// `tprt`: someone scored in Zone War and was moved.
    fn on_teleport(&mut self, z: &Value, gfx: &mut Gfx) {
        let idx = num(z.get("indx")) as u32;
        let (nx, ny) = (num(z.get("newX")), num(z.get("newY")));
        let Some(p) = self.find_mut(idx) else { return };
        p.x = nx;
        p.y = ny;
        let name = p.name.clone();
        self.fx.smoke_puff(nx, ny, 5.0, false, 1.0);
        if Some(idx) == self.me {
            let s = format!(
                "+{} POINTS",
                value_text(z.get("score").unwrap_or(&Value::Null))
            );
            self.big_text(
                gfx,
                "ZONE ENTERED",
                &s,
                2000.0,
                true,
                "#ffffff",
                TEAM_BLUE,
                true,
                1.3,
            );
        } else {
            self.fx
                .smoke_puff(num(z.get("oldX")), num(z.get("oldY")), 5.0, false, 1.0);
            self.notify(&format!("{name} scored"), gfx);
        }
    }

    /// KRP `createSpray`.
    fn create_spray(&mut self, idx: u32, x: f64, y: f64) {
        let Some(info) = self.find(idx).and_then(|p| p.spray.clone()) else {
            return;
        };
        let pos = if let Some(i) = self.sprays.iter().position(|s| s.owner == idx) {
            i
        } else {
            self.sprays.push(SprayState {
                owner: idx,
                src: String::new(),
                active: false,
                x: 0.0,
                y: 0.0,
                scale: 0.0,
                alpha: 0.0,
            });
            self.sprays.len() - 1
        };
        let s = &mut self.sprays[pos];
        s.active = true;
        s.scale = info.info.scale;
        s.alpha = info.info.alpha;
        s.x = x - s.scale / 2.0;
        s.y = y - s.scale / 2.0;
        s.src = info.src;
    }

    /// KRP `messageFromServer` (`cht`).
    fn message_from_server(&mut self, a: &[Value]) {
        let idx = a.first().and_then(Value::as_f64).unwrap_or(-2.0);
        let text = a.get(1).map(value_text).unwrap_or_default();
        let user = (idx >= 0.0)
            .then(|| self.find(idx as u32).cloned())
            .flatten();
        if let Some(u) = user {
            if Some(u.index) == self.me {
                return;
            }
            let source = if u.team == self.me().team {
                "blue"
            } else {
                "red"
            };
            self.add_chat_line(&u.name, &text, source);
        } else if (idx + 1.0).abs() < f64::EPSILON {
            self.add_chat_line("", &text, "system");
        } else {
            self.add_chat_line("", &text, "notif");
        }
    }

    /// KRP `addChatLine`, keeping the last 19 lines.
    pub fn add_chat_line(&mut self, author: &str, text: &str, source: &str) {
        self.chat.push(ChatLine {
            author: author.to_owned(),
            text: text.to_owned(),
            source: source.to_owned(),
        });
        if self.chat.len() >= 20 {
            self.chat.remove(0);
        }
    }

    /// KRP `showStatTable` after a round (the `"7"` event).
    fn show_stat_table(&mut self, votes: &Value, winner: &Value, fading: bool, gfx: &mut Gfx) {
        let me = self.me().clone();
        let is_winner =
            winner.as_str() == Some(me.team.as_str()) || (winner.is_number() && winner == &me.id);
        if !fading {
            if is_winner {
                self.big_text(
                    gfx,
                    "Victory",
                    "Well Played!",
                    2500.0,
                    true,
                    TEAM_BLUE,
                    "#ffffff",
                    false,
                    2.0,
                );
                self.winner_text = Some(("VICTORY".into(), TEAM_BLUE.into()));
            } else if !me.team.is_empty() {
                self.big_text(
                    gfx,
                    "Defeat",
                    "Bad Luck!",
                    2500.0,
                    true,
                    TEAM_RED,
                    "#ffffff",
                    false,
                    2.0,
                );
                self.winner_text = Some(("DEFEAT".into(), TEAM_RED.into()));
            }
        }
        if let Some(list) = votes.as_array() {
            self.votes = list
                .iter()
                .map(|v| {
                    (
                        v.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        num(v.get("votes")),
                    )
                })
                .collect();
            self.my_vote = None;
        }
        if fading {
            self.overlay_alpha = super::OVERLAY_MAX_ALPHA;
            self.animate_overlay = false;
            self.game_over_fade = true;
            self.anim.clear();
            self.stat_table = true;
        } else {
            self.hide_stat_table(gfx);
            self.animate_overlay = true;
            self.after(2500.0, TimerKind::GameOverFade);
            self.after(4500.0, TimerKind::ShowStatTable);
        }
    }
}

/// A JSON value as JavaScript would put it in a string.
fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), fmt_num),
        Value::Null => "undefined".into(),
        other => other.to_string(),
    }
}

/// A number as JavaScript prints it (`5`, not `5.0`).
#[must_use]
pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_javascript() {
        assert_eq!(fmt_num(5.0), "5");
        assert_eq!(fmt_num(-2.5), "-2.5");
        assert_eq!(value_text(&json!("x")), "x");
    }
}

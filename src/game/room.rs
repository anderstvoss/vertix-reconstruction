//! One room: KRP's `Room` and `Game` (`server/room.ts`, `server/game.ts`).
//!
//! A room owns its players, round, map and bullets. It reacts to client
//! events and to time, and queues what to send in [`Room::out`]; the game
//! loop delivers it. Nothing here touches the network, so the rules are
//! testable directly.
//!
//! The port follows KRP closely so the game feels the same. Where it
//! knowingly differs (input limits, per-player weapons, one welcome per
//! player at round start), the code says so and docs/DEVIATIONS.md lists it.

use std::collections::HashMap;
use std::f64::consts::PI;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::{Map as JsonMap, Value, json};

use super::assumptions::{Rules, World as Screen};
use super::data::{GameData, Mode};
use super::map::{Body, Map, World, dot_in_rect};
use super::maps::MapSet;
use super::projectile::{Owner, Projectile, Shot, Target};
use crate::sio::Event;

mod admin;

/// Who an outgoing event is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum To {
    /// Everyone connected to the room.
    All,
    /// One player, by index.
    One(u32),
    /// Every player on a team.
    Team(String),
}

#[derive(Debug, Clone)]
pub struct Out {
    pub to: To,
    pub event: Event,
}

/// Something the room must do later.
#[derive(Debug, Clone)]
enum Timer {
    SpawnProtectionEnd { player: u32, life: u64 },
    Reload { player: u32, slot: usize },
    HealthpackBack { pickup: usize, round: u64 },
    KillStreakEnd { player: u32, streak: u32 },
    Countdown { left: i64 },
    LootCheck,
}

// Flags mirror KRP's object fields one for one.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Player {
    pub index: u32,
    pub name: String,
    pub class_index: usize,
    pub team: String,
    pub weapons: Vec<JsonMap<String, Value>>,
    pub weapon_ids: Vec<usize>,
    pub spread_index: Vec<usize>,
    pub current_weapon: usize,
    pub health: f64,
    pub max_health: f64,
    pub width: f64,
    pub height: f64,
    pub speed: f64,
    pub jump_y: f64,
    pub jump_delta: f64,
    pub jump_strength: f64,
    pub gravity_strength: f64,
    pub jump_countdown: f64,
    pub score_countdown: f64,
    pub kills: u32,
    pub deaths: u32,
    pub score: f64,
    pub angle: f64,
    pub x: f64,
    pub y: f64,
    pub old_x: f64,
    pub old_y: f64,
    pub total_damage: f64,
    pub total_healing: f64,
    pub total_goals: u32,
    pub damage_sources: HashMap<u32, f64>,
    pub kill_streak: u32,
    pub spawn_protected: bool,
    pub name_y_offset: f64,
    pub dead: bool,
    pub on_screen: bool,
    pub delta: f64,
    pub target_f: f64,
    pub is_boss: bool,
    pub first_receive: bool,
    pub last_mode_vote: Option<usize>,
    pub liked_by: Vec<u32>,
    pub in_hardpoint: bool,
    pub hardpoint_score: f64,
    pub hat: Option<Value>,
    pub shirt: Option<Value>,
    pub spray: Value,
    /// Chosen camo per weapon id (`cCamo`, the camo's index; -1 for none).
    /// Kept apart from `weapons`, which every spawn rebuilds.
    pub camos: HashMap<usize, f64>,
    /// Bumped on every spawn, so a stale spawn-protection timer is ignored.
    life: u64,
}

#[derive(Debug, Clone)]
struct ModeVote {
    name: String,
    indx: usize,
    votes: i64,
}

/// One room's whole state.
pub struct Room {
    pub name: String,
    pub players: Vec<Player>,
    pub mode_index: usize,
    pub world: World,
    pub round_end: bool,
    pub score_red: f64,
    pub score_blue: f64,
    /// The leader's progress to the score limit, in percent (room list).
    pub score_lb: f64,
    /// The map the round is played on (`custom` for the custom server form's).
    pub map_id: String,
    /// Score limit set from the admin console; the mode's own when `None`.
    pub score_limit: Option<f64>,
    /// Players the room takes now; the custom server form can lower it.
    pub max_players: usize,
    /// The server's limit for this room; the form cannot go above it.
    pub player_limit: usize,
    screen: [f64; 3],
    mults_health: f64,
    mults_speed: f64,
    mode_votes: Vec<ModeVote>,
    bullets: Vec<Projectile>,
    next_bullet: usize,
    next_index: u32,
    round: u64,
    timers: Vec<(f64, Timer)>,
    rng: StdRng,
    pub out: Vec<Out>,
}

fn ev(name: &str, args: Vec<Value>) -> Event {
    Event::new(name, args)
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|f| f.is_finite())
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Strips markup and control characters, trims and caps a name.
fn clean_text(raw: &str, max: usize) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag && !c.is_control() => out.push(c),
            _ => {}
        }
    }
    out.trim().chars().take(max).collect()
}

impl Room {
    /// A room starting on mode `mode_index`.
    #[must_use]
    pub fn new(
        name: &str,
        data: &GameData,
        maps: &MapSet,
        rules: &Rules,
        screen: &Screen,
        mode_index: usize,
        seed: u64,
    ) -> Self {
        let scale = screen.tile_scale;
        let mut rng = StdRng::seed_from_u64(seed);
        let mode_index = mode_index.min(data.modes.len() - 1);
        let (map_id, map) = maps.pick(&data.modes[mode_index], rng.random());
        let world = World::new(&map, scale, &data.modes[mode_index], &mut |lo, hi| {
            rng.random_range(lo..=hi)
        });
        let mut room = Self {
            name: name.to_owned(),
            players: Vec::new(),
            mode_index,
            world,
            round_end: false,
            score_red: 0.0,
            score_blue: 0.0,
            score_lb: 0.0,
            map_id,
            score_limit: None,
            max_players: rules.max_players,
            player_limit: rules.max_players,
            screen: [
                screen.max_screen_width,
                screen.max_screen_height,
                screen.view_mult,
            ],
            mults_health: 1.0,
            mults_speed: 1.0,
            mode_votes: Vec::new(),
            bullets: Vec::new(),
            next_bullet: 0,
            next_index: 0,
            round: 0,
            timers: Vec::new(),
            rng,
            out: Vec::new(),
        };
        room.reset_votes(data, None);
        room.new_round(data, maps, rules, mode_index, None, 0.0);
        room.timers.push((rules.loot_interval_ms, Timer::LootCheck));
        room
    }

    #[must_use]
    pub fn mode<'a>(&self, data: &'a GameData) -> &'a Mode {
        &data.modes[self.mode_index]
    }

    fn send(&mut self, to: To, name: &str, args: Vec<Value>) {
        self.out.push(Out {
            to,
            event: ev(name, args),
        });
    }

    fn reset_votes(&mut self, data: &GameData, only: Option<&[usize]>) {
        self.mode_votes = data
            .modes
            .iter()
            .enumerate()
            .filter(|(i, _)| only.is_none_or(|o| o.contains(i)))
            .map(|(i, m)| ModeVote {
                name: m.name.clone(),
                indx: i,
                votes: 0,
            })
            .collect();
    }

    fn votes_json(&self) -> Value {
        Value::Array(
            self.mode_votes
                .iter()
                .map(|v| json!({"name": v.name, "indx": v.indx, "votes": v.votes}))
                .collect(),
        )
    }

    fn player(&self, index: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.index == index)
    }

    fn player_mut(&mut self, index: u32) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.index == index)
    }

    /// KRP `getTeam`.
    fn team_for(&self, data: &GameData, index: u32) -> String {
        let mode = self.mode(data);
        if !mode.teams {
            return index.to_string();
        }
        let red = self.players.iter().filter(|p| p.team == "red").count();
        let blue = self.players.iter().filter(|p| p.team == "blue").count();
        let t = if mode.code == "boss" {
            if blue > 0 { "red" } else { "blue" }
        } else if red >= blue {
            "blue"
        } else {
            "red"
        };
        t.to_owned()
    }

    /// KRP `newRound`.
    pub fn new_round(
        &mut self,
        data: &GameData,
        maps: &MapSet,
        rules: &Rules,
        mode_index: usize,
        custom_map: Option<Map>,
        _now: f64,
    ) {
        self.round += 1;
        self.score_red = 0.0;
        self.score_blue = 0.0;
        self.score_lb = 0.0;
        self.mode_index = mode_index.min(data.modes.len() - 1);
        let mode = data.modes[self.mode_index].clone();
        let map = if let Some(m) = custom_map {
            "custom".clone_into(&mut self.map_id);
            m
        } else {
            let (id, m) = maps.pick(&mode, self.rng.random());
            self.map_id = id;
            m
        };
        let rng = &mut self.rng;
        self.world = World::new(&map, self.world.scale, &mode, &mut |lo, hi| {
            rng.random_range(lo..=hi)
        });
        for v in &mut self.mode_votes {
            v.votes = 0;
        }
        for i in 0..self.players.len() {
            let index = self.players[i].index;
            let p = &mut self.players[i];
            p.x = 0.0;
            p.y = 0.0;
            p.score = 0.0;
            p.kills = 0;
            p.deaths = 0;
            p.total_damage = 0.0;
            p.total_healing = 0.0;
            p.total_goals = 0;
            p.is_boss = false;
            p.on_screen = false;
            p.dead = true;
            p.last_mode_vote = None;
            p.first_receive = true;
            p.liked_by.clear();
            // KRP assigns teams one by one, counting the ones already done.
            p.team = String::new();
            let team = self.team_for(data, index);
            self.players[i].team = team;
        }
        self.bullets = (0..rules.bullet_pool)
            .map(|i| Projectile {
                server_index: i,
                ..Projectile::default()
            })
            .collect();
        self.timers
            .retain(|(_, t)| matches!(t, Timer::LootCheck | Timer::Reload { .. }));
        self.round_end = false;
    }

    /// Sets the server's player limit for this room (at least 1).
    pub fn set_player_limit(&mut self, limit: usize) {
        self.player_limit = limit.max(1);
        self.max_players = self.player_limit;
    }

    /// A client joined the room's namespace (KRP `connection`).
    #[allow(clippy::too_many_lines)]
    pub fn join(&mut self, data: &GameData) -> u32 {
        let index = self.next_index;
        self.next_index += 1;
        let team = self.team_for(data, index);
        let spray = data
            .cosmetics
            .sprays
            .first()
            .cloned()
            .unwrap_or(Value::Null);
        let player = Player {
            index,
            name: "UNKNOWN".into(),
            class_index: 0,
            team,
            weapons: Vec::new(),
            weapon_ids: Vec::new(),
            spread_index: Vec::new(),
            current_weapon: 0,
            health: 0.0,
            max_health: 0.0,
            width: 50.0,
            height: 100.0,
            speed: 0.5,
            jump_y: 0.0,
            jump_delta: 0.0,
            jump_strength: 0.0,
            gravity_strength: 0.0,
            jump_countdown: 0.0,
            score_countdown: 0.0,
            kills: 0,
            deaths: 0,
            score: 0.0,
            angle: 0.0,
            x: 0.0,
            y: 0.0,
            old_x: 0.0,
            old_y: 0.0,
            total_damage: 0.0,
            total_healing: 0.0,
            total_goals: 0,
            damage_sources: HashMap::new(),
            kill_streak: 0,
            spawn_protected: false,
            name_y_offset: 0.0,
            dead: true,
            on_screen: false,
            delta: 0.0,
            target_f: 0.0,
            is_boss: false,
            first_receive: true,
            last_mode_vote: None,
            liked_by: Vec::new(),
            in_hardpoint: false,
            hardpoint_score: 0.0,
            hat: None,
            shirt: None,
            spray: with_src(&spray),
            camos: HashMap::new(),
            life: 0,
        };
        self.players.push(player);
        self.set_class(data, index, 0);
        let me = To::One(index);
        self.send(me.clone(), "yourRoom", vec![json!(self.name)]);
        let welcome = self.welcome_json(index);
        self.send(me.clone(), "welcome", vec![welcome, json!(true)]);
        let c = &data.cosmetics;
        let visible = |list: &[Value], keys: &[&str]| -> Vec<Value> {
            let mut v: Vec<Value> = list
                .iter()
                .filter(|h| !h.get("hide").and_then(Value::as_bool).unwrap_or(false))
                .map(|h| {
                    let mut o = JsonMap::new();
                    for k in keys {
                        if let Some(x) = h.get(*k) {
                            o.insert((*k).to_owned(), x.clone());
                        }
                    }
                    o.insert("count".into(), json!(0));
                    Value::Object(o)
                })
                .collect();
            v.sort_by(|a, b| {
                let ch = |x: &Value| x.get("chance").and_then(Value::as_f64).unwrap_or(0.0);
                ch(a).total_cmp(&ch(b))
            });
            v
        };
        let hats = visible(
            &c.hats,
            &[
                "id", "name", "desc", "chance", "creator", "left", "up", "nameY",
            ],
        );
        let shirts = visible(&c.shirts, &["id", "name", "desc", "chance", "left", "up"]);
        let camos = visible(&c.camos, &["id", "name", "chance"]);
        self.send(me.clone(), "updHt", vec![json!(c.hats.len()), json!(hats)]);
        self.send(
            me.clone(),
            "updShrt",
            vec![json!(c.shirts.len()), json!(shirts)],
        );
        let per_weapon: Vec<Value> = data.weapons.iter().map(|_| json!(camos)).collect();
        self.send(me, "updCmo", vec![json!(c.camos.len()), json!(per_weapon)]);
        index
    }

    fn welcome_json(&self, index: u32) -> Value {
        let p = self.player(index);
        json!({
            "id": index,
            "room": self.name,
            "name": p.map_or("", |p| p.name.as_str()),
            "classIndex": p.map_or(0, |p| p.class_index),
        })
    }

    fn set_class(&mut self, data: &GameData, index: u32, class_req: usize) {
        let (ci, class) = data.class(class_req);
        let class = class.clone();
        let (mh, ms) = (self.mults_health, self.mults_speed);
        let Some(p) = self.player_mut(index) else {
            return;
        };
        p.class_index = ci;
        p.current_weapon = 0;
        p.weapon_ids.clone_from(&class.weapon_indexes);
        p.weapons = class
            .weapon_indexes
            .iter()
            .map(|&w| {
                let mut o = data.weapons[w].json.clone();
                // Each player carries their own camo (KRP shares one per room).
                o.insert(
                    "camo".into(),
                    json!(p.camos.get(&w).copied().unwrap_or(-1.0)),
                );
                o
            })
            .collect();
        p.spread_index = vec![0; class.weapon_indexes.len()];
        p.max_health = class.max_health * mh;
        p.health = p.max_health;
        p.height = class.height;
        p.width = class.width;
        p.speed = class.speed * ms;
        p.jump_strength = class.jump_strength;
        p.gravity_strength = class.gravity_strength;
    }

    /// The player object as the client stores it (`add`, `gameSetup`).
    #[must_use]
    pub fn player_json(&self, p: &Player) -> Value {
        let mut account = json!({"clan": "", "rank": 0});
        if let Some(h) = &p.hat {
            account["hat"] = h.clone();
        }
        if let Some(s) = &p.shirt {
            account["shirt"] = s.clone();
        }
        let weapons: Vec<Value> = p
            .weapons
            .iter()
            .zip(&p.spread_index)
            .map(|(w, &si)| {
                let mut o = w.clone();
                o.insert("spreadIndex".into(), json!(si));
                Value::Object(o)
            })
            .collect();
        let sources: JsonMap<String, Value> = p
            .damage_sources
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();
        json!({
            "id": p.index,
            "room": self.name,
            "index": p.index,
            "name": p.name,
            "account": account,
            "classIndex": p.class_index,
            "currentWeapon": p.current_weapon,
            "weapons": weapons,
            "health": p.health,
            "maxHealth": p.max_health,
            "height": p.height,
            "width": p.width,
            "speed": p.speed,
            "jumpY": p.jump_y,
            "jumpDelta": p.jump_delta,
            "jumpStrength": p.jump_strength,
            "gravityStrength": p.gravity_strength,
            "jumpCountdown": p.jump_countdown,
            "frameCountdown": 0,
            "scoreCountdown": p.score_countdown,
            "kills": p.kills,
            "deaths": p.deaths,
            "score": p.score,
            "angle": p.angle,
            "x": p.x,
            "y": p.y,
            "oldX": p.old_x,
            "oldY": p.old_y,
            "totalDamage": p.total_damage,
            "totalHealing": p.total_healing,
            "totalGoals": p.total_goals,
            "damageSources": sources,
            "killStreak": p.kill_streak,
            "isSpawnProtected": p.spawn_protected,
            "nameYOffset": p.name_y_offset,
            "dead": p.dead,
            "onScreen": p.on_screen,
            "delta": p.delta,
            "targetF": p.target_f,
            "animIndex": 0,
            "team": p.team,
            "firstReceive": p.first_receive,
            "isBoss": p.is_boss,
            "spray": p.spray,
            "isInHardpoint": p.in_hardpoint,
            "hardpointScore": p.hardpoint_score,
            "likedBy": p.liked_by,
        })
    }

    /// Positions for `rsd`: `[kind, index, x, y, angle, extra]` per player
    /// (kind 6), or without `extra` (kind 5).
    fn rsd(&self, me: Option<(u32, i64)>) -> Value {
        let mut flat = Vec::with_capacity(self.players.len() * 6);
        for p in &self.players {
            if let Some((my, isn)) = me {
                flat.extend([
                    json!(6),
                    json!(p.index),
                    json!(p.x),
                    json!(p.y),
                    json!(p.angle),
                ]);
                if p.index == my {
                    flat.push(json!(isn));
                } else {
                    flat.push(json!(p.name_y_offset));
                }
            } else {
                flat.extend([
                    json!(5),
                    json!(p.index),
                    json!(p.x),
                    json!(p.y),
                    json!(p.angle),
                ]);
            }
        }
        Value::Array(flat)
    }

    /// Handles one client event from player `index` at time `now` (ms).
    #[allow(clippy::too_many_lines)]
    pub fn on_event(
        &mut self,
        data: &GameData,
        maps: &MapSet,
        rules: &Rules,
        index: u32,
        event: &Event,
        now: f64,
    ) {
        let a = &event.args;
        match event.name.as_str() {
            "cHat" => {
                let hat = num(a.first())
                    .and_then(|id| {
                        data.cosmetics
                            .hats
                            .iter()
                            .find(|h| h["id"].as_f64() == Some(id))
                    })
                    .cloned();
                if let Some(p) = self.player_mut(index) {
                    p.hat = hat;
                }
            }
            "cShirt" => {
                let shirt = num(a.first())
                    .and_then(|id| {
                        data.cosmetics
                            .shirts
                            .iter()
                            .find(|h| h["id"].as_f64() == Some(id))
                    })
                    .cloned();
                if let Some(p) = self.player_mut(index) {
                    p.shirt = shirt;
                }
            }
            "cCamo" => {
                let arg = a.first();
                let weapon = num(arg.and_then(|v| v.get("weaponID")));
                let camo = num(arg.and_then(|v| v.get("camoID")));
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                if let (Some(w), Some(c)) = (weapon, camo)
                    && w >= 0.0
                    && w.fract() == 0.0
                    && (w as usize) < data.weapons.len()
                    && let Some(p) = self.player_mut(index)
                {
                    // Remembered per weapon, so it survives respawns and
                    // class changes: KRP's client sends it once.
                    let w = w as usize;
                    p.camos.insert(w, c - 1.0);
                    for (slot, &id) in p.weapon_ids.iter().enumerate() {
                        if id == w {
                            p.weapons[slot].insert("camo".into(), json!(c - 1.0));
                        }
                    }
                }
            }
            "cSpray" => {
                let spray = num(a.first()).and_then(|id| {
                    data.cosmetics
                        .sprays
                        .iter()
                        .find(|s| s["id"].as_f64() == Some(id))
                });
                if let Some(s) = spray.map(with_src)
                    && let Some(p) = self.player_mut(index)
                {
                    p.spray = s;
                }
            }
            "gotit" => self.on_gotit(data, rules, index, a, now),
            "respawn" => {
                let w = self.welcome_json(index);
                self.send(To::One(index), "welcome", vec![w, json!(false)]);
            }
            "sw" => {
                let Some(slot) = num(a.first()) else { return };
                let Some(p) = self.player_mut(index) else {
                    return;
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let slot = slot as usize;
                if slot >= p.weapons.len() {
                    return;
                }
                p.current_weapon = slot;
                self.send(To::All, "upd", vec![json!({"i": index, "wi": slot})]);
            }
            "r" => {
                let Some(p) = self.player_mut(index) else {
                    return;
                };
                let slot = p.current_weapon;
                let Some(&w) = p.weapon_ids.get(slot) else {
                    return;
                };
                p.spread_index[slot] = 0;
                let reload = data.weapons[w].spec.reload_speed;
                self.timers.push((
                    now + reload,
                    Timer::Reload {
                        player: index,
                        slot,
                    },
                ));
            }
            "0" => {
                if let Some(f) = num(a.first())
                    && let Some(p) = self.player_mut(index)
                {
                    p.target_f = f;
                }
            }
            "1" => self.on_fire(data, index, a, now),
            "4" => self.on_input(data, rules, index, a.first(), now),
            "cht" => {
                let Some(raw) = a.first().and_then(Value::as_str) else {
                    return;
                };
                let msg = clean_text(raw, rules.chat_max_len);
                if msg.is_empty() {
                    return;
                }
                if msg.contains("!sync") {
                    let rsd = self.rsd(None);
                    self.send(To::All, "rsd", vec![rsd]);
                    self.send(To::One(index), "cht", vec![json!([-1, "synced"])]);
                    return;
                }
                let team = self
                    .player(index)
                    .map(|p| p.team.clone())
                    .unwrap_or_default();
                if a.get(1).and_then(Value::as_str) == Some("TEAM") && self.mode(data).teams {
                    self.send(
                        To::Team(team),
                        "cht",
                        vec![json!([index, format!("(TEAM) {msg}")])],
                    );
                } else {
                    self.send(To::All, "cht", vec![json!([index, msg])]);
                }
            }
            "modeVote" => {
                let Some(i) = num(a.first()) else { return };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let i = i as usize;
                if i >= self.mode_votes.len()
                    || i.to_string() != a[0].to_string().trim_end_matches(".0")
                {
                    return;
                }
                let last = self.player(index).and_then(|p| p.last_mode_vote);
                if let Some(l) = last
                    && let Some(v) = self.mode_votes.get_mut(l)
                {
                    v.votes -= 1;
                    let msg = json!({"i": l, "n": v.name, "v": v.votes});
                    self.send(To::All, "vt", vec![msg]);
                }
                if let Some(p) = self.player_mut(index) {
                    p.last_mode_vote = Some(i);
                }
                let v = &mut self.mode_votes[i];
                v.votes += 1;
                let msg = json!({"i": i, "n": v.name, "v": v.votes});
                self.send(To::All, "vt", vec![msg]);
            }
            "like" => {
                let Some(dest) = num(a.get(1)) else { return };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let dest = dest as u32;
                // The liker is always the sender (KRP trusts the argument).
                let Some(p) = self.player_mut(dest) else {
                    return;
                };
                if let Some(at) = p.liked_by.iter().position(|&i| i == index) {
                    p.liked_by.remove(at);
                } else {
                    p.liked_by.push(index);
                }
                let l = p.liked_by.clone();
                self.send(To::All, "upd", vec![json!({"i": dest, "l": l})]);
            }
            "ping1" => self.send(To::One(index), "pong1", vec![]),
            "cSrv" => self.on_custom_server(data, maps, rules, a.first(), now),
            "crtSpr" => {
                let Some(p) = self.player(index) else { return };
                let (mut y_off, mut muzzle) = (55.0, 50.0);
                if let Some(&w) = p.weapon_ids.get(p.current_weapon) {
                    let s = &data.weapons[w].spec;
                    y_off = s.y_offset;
                    muzzle = s.hold_dist + s.b_dist;
                }
                let ang = p.target_f + PI;
                let x = (p.x + muzzle * ang.cos()).round();
                let y = (p.y - p.jump_y - y_off / 2.0 + muzzle * ang.sin()).round();
                self.send(To::All, "crtSpr", vec![json!(index), json!(x), json!(y)]);
            }
            "ftc" => {
                // The client asks for a player it has not heard of.
                if let Some(i) = num(a.first()) {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    if let Some(p) = self.player(i as u32) {
                        let add = self.player_json(p).to_string();
                        self.send(To::One(index), "add", vec![json!(add)]);
                    }
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn on_gotit(&mut self, data: &GameData, rules: &Rules, index: u32, a: &[Value], now: f64) {
        let client = a.first();
        let init = a.get(1).and_then(Value::as_bool).unwrap_or(false);
        let name = client
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
            .map(|n| clean_text(n, rules.name_max_len))
            .filter(|n| !n.is_empty());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let mut class_req = num(client.and_then(|c| c.get("classIndex")))
            .filter(|c| *c >= 0.0)
            .map_or(0, |c| c as usize);
        let code = self.mode(data).code.clone();
        let team = self
            .player(index)
            .map(|p| p.team.clone())
            .unwrap_or_default();
        let forced = |n: &str| {
            data.classes
                .iter()
                .position(|c| c.name.as_deref() == Some(n))
        };
        let mut boss = false;
        match code.as_str() {
            "snipe" => class_req = forced("Hunter").unwrap_or(2),
            "rckt" => class_req = forced("Rocketeer").unwrap_or(5),
            "pyro" => class_req = forced("Arsonist").unwrap_or(7),
            "boss" if team == "blue" => {
                class_req = forced("???").unwrap_or(10);
                boss = true;
            }
            _ => {}
        }
        if let Some(p) = self.player_mut(index) {
            if let Some(n) = name {
                p.name = n;
            }
            p.is_boss = boss;
        }
        // The boss class is not offered in the menu, so `available` must
        // not turn it away here.
        if boss {
            self.set_class_unchecked(data, index, class_req);
        } else {
            self.set_class(data, index, class_req);
        }
        if init {
            return;
        }
        let spawn = self.spawn_point(data, index);
        let protect = rules.spawn_protection_ms;
        let Some(p) = self.player_mut(index) else {
            return;
        };
        p.on_screen = true;
        p.angle = 0.0;
        p.x = spawn.0;
        p.y = spawn.1;
        p.dead = false;
        p.spawn_protected = true;
        p.in_hardpoint = false;
        p.damage_sources.clear();
        p.jump_y = 0.0;
        p.jump_delta = 0.0;
        p.life += 1;
        let life = p.life;
        let first = std::mem::replace(&mut p.first_receive, false);
        let is_boss = p.is_boss;
        self.timers.push((
            now + protect,
            Timer::SpawnProtectionEnd {
                player: index,
                life,
            },
        ));
        let Some(p) = self.player(index) else { return };
        let users: Vec<Value> = self.players.iter().map(|u| self.player_json(u)).collect();
        let w = self.screen;
        let setup = json!({
            "mapData": self.world.client_json(self.mode(data)),
            "maxScreenWidth": w[0],
            "maxScreenHeight": w[1],
            "viewMult": w[2],
            "tileScale": self.world.scale,
            "usersInRoom": users,
            "you": self.player_json(p),
        });
        let add = self.player_json(p).to_string();
        let me = To::One(index);
        self.send(
            me.clone(),
            "gameSetup",
            vec![json!(setup.to_string()), json!(true), json!(true)],
        );
        if first {
            let mode = self.mode(data);
            let desc = if is_boss { &mode.desc2 } else { &mode.desc1 };
            let msg = vec![json!(mode.name), json!(desc), json!(1.25)];
            self.send(me.clone(), "6", msg);
        }
        self.send(To::All, "add", vec![json!(add)]);
        let rsd = self.rsd(None);
        self.send(me.clone(), "rsd", vec![rsd]);
        if self.round_end {
            let votes = self.votes_json();
            let t = self
                .player(index)
                .map(|p| p.team.clone())
                .unwrap_or_default();
            self.send(me, "7", vec![json!(t), votes, json!(false)]);
        } else {
            self.update_score(data, rules, 0.0, index, now);
        }
    }

    fn set_class_unchecked(&mut self, data: &GameData, index: u32, class: usize) {
        if class < data.classes.len() && !data.classes[class].available {
            let mut d = data.clone();
            d.classes[class].available = true;
            self.set_class(&d, index, class);
        } else {
            self.set_class(data, index, class);
        }
    }

    /// KRP `getSpawn`: the spawn tile farthest from the nearest enemy.
    fn spawn_point(&self, data: &GameData, index: u32) -> (f64, f64) {
        let teams = self.mode(data).teams;
        let Some(me) = self.player(index) else {
            return (0.0, 0.0);
        };
        let mid = self.world.scale / 2.0;
        let enemies: Vec<&Player> = self
            .players
            .iter()
            .filter(|p| !p.dead && p.on_screen && p.index != index && (!teams || p.team != me.team))
            .collect();
        let mut best = (mid, mid);
        let mut best_d = f64::NEG_INFINITY;
        for &i in &self.world.spawn_tiles {
            let t = &self.world.tiles[i];
            if teams && t.obj_team != me.team {
                continue;
            }
            let pos = (t.x + mid, t.y + mid);
            let d = enemies
                .iter()
                .map(|p| (p.x - pos.0).hypot(p.y - pos.1))
                .fold(f64::INFINITY, f64::min);
            if d > best_d {
                best = pos;
                best_d = d;
            }
        }
        best
    }

    #[allow(clippy::many_single_char_names)]
    fn on_fire(&mut self, data: &GameData, index: u32, a: &[Value], now: f64) {
        let (Some(x), Some(y), Some(jump_y), Some(target_f), Some(target_d)) = (
            num(a.first()),
            num(a.get(1)),
            num(a.get(2)),
            num(a.get(3)),
            num(a.get(4)),
        ) else {
            return;
        };
        let Some(p) = self.player(index) else { return };
        if p.dead {
            return;
        }
        let slot = p.current_weapon;
        let Some(&w) = p.weapon_ids.get(slot) else {
            return;
        };
        let spec = data.weapons[w].spec.clone();
        let owner = Owner {
            index,
            team: p.team.clone(),
            height: p.height,
        };
        let p_jump = p.jump_y;
        for _ in 0..spec.bullets_per_shot {
            let Some(p) = self.player_mut(index) else {
                return;
            };
            let mut si = p.spread_index[slot] + 1;
            if si >= spec.spread.len() {
                si = 0;
            }
            p.spread_index[slot] = si;
            let spread = spec.spread[si];
            let dir = round2(target_f + PI + spread);
            let origin = spec.hold_dist + spec.b_dist;
            let nx = (x + origin * dir.cos()).round();
            let ny = (y - spec.y_offset - jump_y + origin * dir.sin()).round();
            self.next_bullet = (self.next_bullet + 1) % self.bullets.len();
            let bi = self.next_bullet;
            let next_shot = Shot {
                x: nx,
                y: ny,
                dir,
                server_index: bi,
            };
            self.send(
                To::All,
                "2",
                vec![json!({"i": index, "x": nx, "y": ny, "d": dir, "si": bi})],
            );
            let rand_scale = spec.b_rand_scale.map(|[lo, hi]| {
                if hi > lo {
                    self.rng.random_range(lo..hi)
                } else {
                    lo
                }
            });
            self.bullets[bi].shoot(
                next_shot,
                &spec,
                spread,
                owner.clone(),
                p_jump,
                target_d,
                now,
                rand_scale,
            );
        }
    }

    #[allow(clippy::too_many_lines)]
    fn on_input(
        &mut self,
        data: &GameData,
        rules: &Rules,
        index: u32,
        input: Option<&Value>,
        now: f64,
    ) {
        let Some(input) = input else { return };
        let isn = input.get("isn").and_then(Value::as_i64).unwrap_or(0);
        let (Some(mut h), Some(mut v)) = (num(input.get("hdt")), num(input.get("vdt"))) else {
            return;
        };
        // KRP moves by the client's own frame delta; we cap it so a client
        // cannot move further than a slow frame would allow (NEW limit).
        let delta = num(input.get("delta")).unwrap_or(0.0).clamp(0.0, 100.0);
        let space = num(input.get("s")).is_some_and(|s| (s - 1.0).abs() < f64::EPSILON);
        let mut landed_duck = false;
        let Some(p) = self.player_mut(index) else {
            return;
        };
        if !p.dead {
            p.delta = delta;
            let len = h.hypot(v);
            if len != 0.0 {
                h /= len;
                v /= len;
            }
            p.old_x = p.x;
            p.old_y = p.y;
            p.x += h * p.speed * delta;
            p.y += v * p.speed * delta;
            p.angle = ((p.target_f + PI * 2.0) % (PI * 2.0)) * (180.0 / PI) + 90.0;
            let mut jumped = false;
            if space {
                jumped = true;
                if p.jump_y <= 0.0 {
                    p.jump_delta = p.jump_strength;
                    p.jump_y = p.jump_delta;
                }
            }
            if p.jump_countdown > 0.0 {
                p.jump_countdown -= delta;
            }
            if p.jump_y > 0.0 {
                p.jump_delta -= p.gravity_strength * delta;
                p.jump_y += p.jump_delta * delta;
                if p.jump_y <= 0.0 {
                    p.jump_y = 0.0;
                    p.jump_delta = 0.0;
                    p.jump_countdown = 250.0;
                    landed_duck = data.classes[p.class_index].name.as_deref() == Some("Duck");
                }
                p.jump_y = p.jump_y.round();
            }
            if jumped {
                self.send(To::All, "jum", vec![json!(index)]);
            }
            if landed_duck {
                let p = self.player(index).cloned();
                if let Some(p) = p {
                    let dir = round2(p.target_f + PI);
                    self.explode(
                        data,
                        rules,
                        index,
                        (p.x, p.y),
                        rules.duck_hit_radius,
                        rules.duck_hit_damage,
                        dir,
                        false,
                        None,
                        now,
                    );
                    self.hit(data, rules, index, index, -100.0, dir, None, now);
                }
            }
            let world = &self.world;
            if let Some(p) = self.players.iter_mut().find(|p| p.index == index)
                && !p.dead
            {
                let mut b = Body {
                    x: p.x,
                    y: p.y,
                    old_x: p.old_x,
                    old_y: p.old_y,
                    width: p.width,
                    height: p.height,
                    jump_y: p.jump_y,
                };
                p.name_y_offset = world.wall_col(&mut b);
                p.x = b.x;
                p.y = b.y;
            }
            if let Some(p) = self.player_mut(index) {
                p.x = p.x.round();
                p.y = p.y.round();
            }
        }
        let rsd = self.rsd(Some((index, isn)));
        self.send(To::One(index), "rsd", vec![rsd]);
    }

    /// KRP `checkSpecialTiles`: pickups, hardpoints and zones under a player.
    ///
    /// KRP runs this from each input message and counts the hardpoint
    /// interval down by the client's frame delta, so a player whose tab is
    /// hidden (no frames, no input) stops scoring. Here it runs on the
    /// server tick and counts down by server time (`dt`).
    #[allow(clippy::too_many_lines)]
    fn special_tiles(&mut self, data: &GameData, rules: &Rules, index: u32, dt: f64, now: f64) {
        let Some(p) = self.player(index).cloned() else {
            return;
        };
        if p.dead {
            return;
        }
        for i in 0..self.world.pickups.len() {
            let pk = &self.world.pickups[i];
            if !pk.active
                || !dot_in_rect(
                    p.x,
                    p.y,
                    pk.x - pk.scale / 2.0,
                    pk.y - pk.scale / 2.0,
                    pk.scale,
                    pk.scale,
                )
            {
                continue;
            }
            if pk.kind == "healthpack" && p.health < p.max_health && !p.is_boss {
                let heal = rules.healthpack_heal.min(p.max_health - p.health);
                let Some(pm) = self.player_mut(index) else {
                    return;
                };
                pm.total_healing += heal;
                pm.damage_sources.clear();
                pm.health += heal;
                let (th, hl) = (pm.total_healing, pm.health);
                self.send(To::All, "upd", vec![json!({"i": index, "hea": th})]);
                self.send(
                    To::All,
                    "1",
                    vec![json!({"gID": index, "healthDelta": heal, "health": hl})],
                );
                let round = self.round;
                self.timers.push((
                    now + rules.healthpack_respawn_ms,
                    Timer::HealthpackBack { pickup: i, round },
                ));
            } else if pk.kind == "lootcrate" && self.mode(data).code == "lc" {
                let pts = f64::from(rules.lootcrate_points);
                self.update_score(data, rules, pts, index, now);
                self.send(
                    To::One(index),
                    "6",
                    vec![
                        json!("Loot Collected"),
                        json!(format!("+{} points", rules.lootcrate_points)),
                        json!(1.25),
                    ],
                );
            } else {
                // KRP returns here, skipping hardpoint and zone checks.
                return;
            }
            self.world.pickups[i].active = false;
            let pk = serde_json::to_value(&self.world.pickups[i]).unwrap_or(Value::Null);
            self.send(To::All, "4", vec![pk, json!(i), json!(0)]);
        }
        let code = self.mode(data).code.clone();
        if code == "hp" {
            let Some(p) = self.player_mut(index) else {
                return;
            };
            if p.score_countdown > 0.0 {
                p.score_countdown -= dt;
            } else {
                p.in_hardpoint = false;
                let (px, py, team) = (p.x, p.y, p.team.clone());
                let tiles: Vec<usize> = self.world.score_tiles.clone();
                for t in tiles {
                    let tl = &self.world.tiles[t];
                    if !dot_in_rect(px, py, tl.x, tl.y, tl.scale, tl.scale) || tl.obj_team == team {
                        continue;
                    }
                    if let Some(p) = self.player_mut(index) {
                        p.score_countdown = rules.hardpoint_interval_ms;
                        p.in_hardpoint = true;
                    }
                    self.update_score(data, rules, f64::from(rules.hardpoint_points), index, now);
                    let goals = self.player(index).map_or(0, |p| p.total_goals);
                    self.send(To::All, "upd", vec![json!({"i": index, "goa": goals})]);
                }
                if let Some(p) = self.player_mut(index)
                    && !p.in_hardpoint
                {
                    p.hardpoint_score = 0.0;
                }
            }
        }
        if code == "zmtch" {
            let tiles: Vec<usize> = self.world.score_tiles.clone();
            for t in tiles {
                let Some(p) = self.player(index) else { return };
                let tl = &self.world.tiles[t];
                if !dot_in_rect(p.x, p.y, tl.x, tl.y, tl.scale, tl.scale) || tl.obj_team == p.team {
                    continue;
                }
                let spawn = self.spawn_point(data, index);
                if let Some(p) = self.player_mut(index) {
                    p.x = spawn.0;
                    p.y = spawn.1;
                    p.total_goals += 1;
                }
                let pts = rules.zone_war_points;
                self.send(
                    To::All,
                    "tprt",
                    vec![json!({"indx": index, "score": pts, "newX": spawn.0, "newY": spawn.1})],
                );
                self.update_score(data, rules, f64::from(pts), index, now);
                let goals = self.player(index).map_or(0, |p| p.total_goals);
                self.send(To::All, "upd", vec![json!({"i": index, "goa": goals})]);
            }
        }
    }

    /// KRP `updateScore`, including the round end.
    fn update_score(&mut self, data: &GameData, rules: &Rules, scored: f64, source: u32, now: f64) {
        let mut team = String::new();
        let mut total = 0.0;
        if let Some(p) = self.player_mut(source) {
            p.score += scored;
            total = p.score;
            team.clone_from(&p.team);
            if p.in_hardpoint {
                p.hardpoint_score += scored;
                let hs = p.hardpoint_score;
                self.send(To::One(source), "5", vec![json!(format!("+{hs}"))]);
            }
        }
        let mut order: Vec<&Player> = self.players.iter().collect();
        order.sort_by(|a, b| b.score.total_cmp(&a.score));
        let lb: Vec<u32> = order.iter().map(|p| p.index).collect();
        self.send(To::All, "lb", vec![json!(lb)]);
        self.send(To::All, "upd", vec![json!({"i": source, "s": total})]);
        if team == "red" {
            self.score_red += scored;
            self.send(
                To::All,
                "ts",
                vec![json!(self.score_red), json!(self.score_blue)],
            );
        } else if team == "blue" {
            self.score_blue += scored;
            self.send(
                To::All,
                "ts",
                vec![json!(self.score_red), json!(self.score_blue)],
            );
        } else {
            self.send(To::All, "ts", vec![]);
        }
        let mode = self.mode(data);
        let limit = self.score_limit.unwrap_or(mode.score).max(1.0);
        let lead = if mode.teams {
            (self.score_red * 100.0 / limit).max(self.score_blue * 100.0 / limit)
        } else {
            self.players
                .iter()
                .map(|p| p.score * 100.0 / limit)
                .fold(0.0, f64::max)
        };
        self.score_lb = lead.min(100.0).round();
        if lead >= 100.0 && !self.round_end {
            self.round_end = true;
            let votes = self.votes_json();
            self.send(To::All, "7", vec![json!(team), votes, json!(false)]);
            self.timers.push((
                now + 1000.0,
                Timer::Countdown {
                    left: i64::from(rules.round_end_countdown_s),
                },
            ));
        }
    }

    /// KRP `handleHit`.
    #[allow(clippy::too_many_arguments)]
    fn hit(
        &mut self,
        data: &GameData,
        rules: &Rules,
        source: u32,
        dest: u32,
        dmg: f64,
        dir: f64,
        bullet: Option<usize>,
        now: f64,
    ) {
        let Some(d) = self.player_mut(dest) else {
            return;
        };
        if d.dead {
            return;
        }
        let capped = dmg.max(-d.health);
        d.health += capped;
        *d.damage_sources.entry(source).or_insert(0.0) -= capped;
        let health = d.health;
        self.send(
            To::All,
            "1",
            vec![json!({
                "dID": source,
                "gID": dest,
                "dir": dir,
                "healthDelta": capped,
                "bulletIndex": bullet,
                "health": health,
            })],
        );
        let Some(s) = self.player_mut(source) else {
            return;
        };
        s.total_damage -= capped;
        let td = s.total_damage;
        self.send(To::All, "upd", vec![json!({"i": source, "dmg": td})]);
        if health <= 0.0 {
            self.kill(data, rules, source, dest, now);
        }
    }

    /// KRP `handleKill` and `handleAssist`.
    fn kill(&mut self, data: &GameData, rules: &Rules, source: u32, dest: u32, now: f64) {
        let mult = self.mode(data).kill_score_mult;
        let Some(d) = self.player_mut(dest) else {
            return;
        };
        d.dead = true;
        d.on_screen = false;
        d.deaths += 1;
        let deaths = d.deaths;
        let d = d.clone();
        self.send(To::All, "upd", vec![json!({"i": dest, "dea": deaths})]);
        let suicide = source == dest;
        if !suicide && let Some(s) = self.player_mut(source) {
            s.kills += 1;
            s.kill_streak += 1;
            let streak = s.kill_streak;
            self.timers.push((
                now + rules.kill_streak_window_ms,
                Timer::KillStreakEnd {
                    player: source,
                    streak,
                },
            ));
        }
        let mut scored = 0.0;
        if d.is_boss && !suicide {
            scored = f64::from(rules.boss_kill_score);
        } else if !d.is_boss {
            let self_dmg = d.damage_sources.get(&dest).copied().unwrap_or(0.0);
            let base = d.max_health - self_dmg;
            let mut assists = 0.0;
            let mut helpers: Vec<(u32, f64)> = d
                .damage_sources
                .iter()
                .filter(|&(&i, &dmg)| dmg > 0.0 && i != source && i != dest)
                .map(|(&i, &dmg)| (i, dmg))
                .collect();
            helpers.sort_by_key(|&(i, _)| i);
            for (helper, dmg) in helpers {
                if self.player(helper).is_none() {
                    continue;
                }
                assists += dmg;
                let s = (100.0 * dmg / base).round() * mult;
                self.send(
                    To::All,
                    "3",
                    vec![json!({"dID": helper, "gID": dest, "sS": s, "kB": false, "ast": true})],
                );
                self.update_score(data, rules, s, helper, now);
            }
            if base > 0.0 {
                scored = ((100.0 * (base - assists)) / base).round() * mult;
            }
        }
        for p in &mut self.players {
            p.damage_sources.insert(dest, 0.0);
        }
        let (sname, streak) = self
            .player(source)
            .map_or((String::new(), 0), |s| (s.name.clone(), s.kill_streak));
        let msg = if suicide {
            format!("{sname} committed suicide")
        } else {
            format!("{sname} killed {}", d.name)
        };
        self.send(To::All, "5", vec![json!(msg)]);
        self.send(
            To::All,
            "3",
            vec![json!({"dID": source, "gID": dest, "sS": scored, "kB": d.is_boss, "kd": streak})],
        );
        self.update_score(data, rules, scored, source, now);
        let kills = self.player(source).map_or(0, |p| p.kills);
        self.send(To::All, "upd", vec![json!({"i": source, "kil": kills})]);
    }

    /// KRP `doExplosion`: splash damage, strongest at the centre.
    #[allow(clippy::too_many_arguments)]
    fn explode(
        &mut self,
        data: &GameData,
        rules: &Rules,
        source: u32,
        at: (f64, f64),
        radius: f64,
        max_dmg: f64,
        dir: f64,
        self_damage: bool,
        bullet: Option<usize>,
        now: f64,
    ) {
        self.send(
            To::All,
            "ex",
            vec![json!(at.0), json!(at.1), json!((radius / 50.0).round())],
        );
        let teams = self.mode(data).teams;
        let src_team = self
            .player(source)
            .map(|p| p.team.clone())
            .unwrap_or_default();
        let victims: Vec<(u32, f64)> = self
            .players
            .iter()
            .filter(|p| {
                !((!self_damage && p.index == source)
                    || (teams && p.index != source && p.team == src_team))
            })
            .map(|p| (p.index, (at.0 - p.x).hypot(at.1 - (p.y - p.jump_y))))
            .filter(|&(_, d)| radius > d)
            .collect();
        for (i, dist) in victims {
            let dmg = (-max_dmg * (1.05 * (radius - dist) / radius).min(1.0)).round();
            self.hit(data, rules, source, i, dmg, dir, bullet, now);
        }
    }

    fn on_custom_server(
        &mut self,
        data: &GameData,
        maps: &MapSet,
        rules: &Rules,
        arg: Option<&Value>,
        now: f64,
    ) {
        let Some(d) = arg else { return };
        // Research #69: the form sends unchecked strings, so clamp all of it.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        if let Some(n) = num(d.get("srvPlayers")).or_else(|| {
            d.get("srvPlayers")
                .and_then(Value::as_str)
                .and_then(|s| s.trim().parse().ok())
        }) {
            // At least 2, at most the server's limit for this room.
            let asked = n.clamp(0.0, 1e6) as usize;
            self.max_players = asked.clamp(2.min(self.player_limit), self.player_limit);
        }
        let mult = |k: &str| {
            num(d.get(k))
                .or_else(|| {
                    d.get(k)
                        .and_then(Value::as_str)
                        .and_then(|s| s.trim().parse().ok())
                })
                .filter(|f: &f64| f.is_finite())
                .map(|f| f.clamp(0.01, 100.0))
        };
        if let Some(m) = mult("srvHealthMult") {
            self.mults_health = m;
        }
        if let Some(m) = mult("srvSpeedMult") {
            self.mults_speed = m;
        }
        let modes: Vec<usize> = d
            .get("srvModes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_u64)
                    .filter_map(|i| usize::try_from(i).ok())
                    .filter(|&i| i < data.modes.len())
                    .collect()
            })
            .unwrap_or_default();
        let mut mode_index = 0;
        if modes.is_empty() {
            self.reset_votes(data, None);
        } else {
            self.reset_votes(data, Some(&modes));
            mode_index = modes[0];
        }
        let custom = d.get("srvMap").and_then(|m| Map::from_gen_data(m).ok());
        self.new_round(data, maps, rules, mode_index, custom, now);
    }

    /// A player's connection went away (KRP `disconnect`).
    pub fn leave(&mut self, data: &GameData, rules: &Rules, index: u32, now: f64) {
        self.send(To::All, "rem", vec![json!(index)]);
        let Some(at) = self.players.iter().position(|p| p.index == index) else {
            return;
        };
        let gone = self.players.remove(at);
        // KRP scores the leaver after removing them: only the boards update.
        self.players.push(gone);
        self.update_score(data, rules, 0.0, index, now);
        self.players.pop();
        let lb: Vec<u32> = {
            let mut order: Vec<&Player> = self.players.iter().collect();
            order.sort_by(|a, b| b.score.total_cmp(&a.score));
            order.iter().map(|p| p.index).collect()
        };
        self.send(To::All, "lb", vec![json!(lb)]);
    }

    /// Advances bullets and timers to `now`, `dt` ms after the last tick.
    pub fn tick(&mut self, data: &GameData, maps: &MapSet, rules: &Rules, now: f64, dt: f64) {
        self.tick_bullets(data, rules, now, dt);
        let alive: Vec<u32> = self
            .players
            .iter()
            .filter(|p| !p.dead)
            .map(|p| p.index)
            .collect();
        for index in alive {
            self.special_tiles(data, rules, index, dt, now);
        }
        self.tick_timers(data, maps, rules, now);
    }

    fn tick_bullets(&mut self, data: &GameData, rules: &Rules, now: f64, dt: f64) {
        for b in 0..self.bullets.len() {
            if !self.bullets[b].active {
                continue;
            }
            let targets: Vec<Target<'_>> = self
                .players
                .iter()
                .map(|p| Target {
                    index: p.index,
                    team: &p.team,
                    x: p.x,
                    y: p.y,
                    width: p.width,
                    height: p.height,
                    jump_y: p.jump_y,
                    on_screen: p.on_screen,
                    dead: p.dead,
                    spawn_protected: p.spawn_protected,
                })
                .collect();
            let mut bullet = std::mem::take(&mut self.bullets[b]);
            bullet.update(dt, now, &self.world.clutter, &self.world.tiles, &targets);
            drop(targets);
            let owner = bullet.owner.index;
            let dir = bullet.fire_dir;
            let si = Some(bullet.server_index);
            if !bullet.active && (bullet.explode_on_death || bullet.collides_with_explosive_clutter)
            {
                if bullet.explode_on_death
                    && let Some(r) = bullet.blast_radius
                {
                    self.explode(
                        data,
                        rules,
                        owner,
                        (bullet.x, bullet.y),
                        r,
                        bullet.dmg,
                        dir,
                        bullet.self_damage,
                        si,
                        now,
                    );
                }
                if bullet.collides_with_explosive_clutter
                    && let Some(&i) = bullet.hit_clutter.first()
                    && self.world.clutter.get(i).is_some_and(|c| c.active)
                {
                    let c = &self.world.clutter[i];
                    let at = (c.x + c.w / 2.0, c.y - c.h / 2.0);
                    self.explode(
                        data,
                        rules,
                        owner,
                        at,
                        rules.explosive_clutter_blast_radius,
                        rules.explosive_clutter_hit_damage,
                        dir,
                        true,
                        None,
                        now,
                    );
                    self.world.clutter[i].active = false;
                    let cv = serde_json::to_value(&self.world.clutter[i]).unwrap_or(Value::Null);
                    self.send(To::All, "4", vec![cv, json!(i), json!(1)]);
                }
            } else {
                for &victim in &bullet.hit_players {
                    self.hit(data, rules, owner, victim, -bullet.dmg, dir, si, now);
                }
            }
            if self.bullets.len() > b {
                self.bullets[b] = bullet;
            }
        }
    }

    fn tick_timers(&mut self, data: &GameData, maps: &MapSet, rules: &Rules, now: f64) {
        loop {
            let Some(at) = self.timers.iter().position(|(due, _)| *due <= now) else {
                return;
            };
            let (due, timer) = self.timers.remove(at);
            match timer {
                Timer::SpawnProtectionEnd { player, life } => {
                    if let Some(p) = self.player_mut(player)
                        && p.life == life
                    {
                        p.spawn_protected = false;
                        self.send(To::All, "upd", vec![json!({"i": player, "sp": false})]);
                    }
                }
                Timer::Reload { player, slot } => {
                    self.send(To::One(player), "r", vec![json!(slot)]);
                }
                Timer::HealthpackBack { pickup, round } => {
                    if round == self.round
                        && let Some(pk) = self.world.pickups.get_mut(pickup)
                    {
                        pk.active = true;
                        let v = serde_json::to_value(&*pk).unwrap_or(Value::Null);
                        self.send(To::All, "4", vec![v, json!(pickup), json!(0)]);
                    }
                }
                Timer::KillStreakEnd { player, streak } => {
                    if let Some(p) = self.player_mut(player)
                        && p.kill_streak == streak
                    {
                        p.kill_streak = 0;
                    }
                }
                Timer::Countdown { left } => {
                    if left >= 0 {
                        self.send(To::All, "8", vec![json!(left)]);
                        self.timers
                            .push((due + 1000.0, Timer::Countdown { left: left - 1 }));
                    } else {
                        let mut sorted = self.mode_votes.clone();
                        sorted.sort_by_key(|v| std::cmp::Reverse(v.votes));
                        let next = sorted.first().map_or(0, |v| v.indx);
                        self.new_round(data, maps, rules, next, None, now);
                        let ids: Vec<u32> = self.players.iter().map(|p| p.index).collect();
                        for i in ids {
                            // KRP sends every player's welcome to everyone.
                            let w = self.welcome_json(i);
                            self.send(To::One(i), "welcome", vec![w, json!(true)]);
                        }
                    }
                }
                Timer::LootCheck => {
                    self.timers
                        .push((due + rules.loot_interval_ms, Timer::LootCheck));
                    if self.round_end || self.mode(data).code != "lc" {
                        continue;
                    }
                    let loot: Vec<usize> = (0..self.world.pickups.len())
                        .filter(|&i| self.world.pickups[i].kind == "lootcrate")
                        .collect();
                    let active = loot
                        .iter()
                        .filter(|&&i| self.world.pickups[i].active)
                        .count();
                    let inactive: Vec<usize> = loot
                        .into_iter()
                        .filter(|&i| !self.world.pickups[i].active)
                        .collect();
                    if active >= rules.max_active_loot || inactive.is_empty() {
                        continue;
                    }
                    let i = inactive[self.rng.random_range(0..inactive.len())];
                    self.world.pickups[i].active = true;
                    let v = serde_json::to_value(&self.world.pickups[i]).unwrap_or(Value::Null);
                    self.send(To::All, "4", vec![v, json!(i), json!(0)]);
                }
            }
        }
    }

    /// The room's entry in `/api/getRooms`.
    #[must_use]
    pub fn list_entry(&self, data: &GameData) -> Value {
        json!({
            "n": self.name,
            "m": self.mode(data).code,
            "pl": self.players.len(),
            "mxpl": self.max_players,
            "lb": self.score_lb,
        })
    }
}

/// A spray with the image path the client loads.
fn with_src(spray: &Value) -> Value {
    let mut s = spray.clone();
    if let Some(o) = s.as_object_mut() {
        let id = o.get("id").cloned().unwrap_or(json!(1));
        o.insert("src".into(), json!(format!("/assets/sprays/{id}.png")));
    }
    s
}

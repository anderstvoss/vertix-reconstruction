//! The game server: rooms, players, and the 2016-08-06 event contract.
//!
//! One task owns all game state and handles transport events and ticks in
//! order, so there are no locks in game logic. Everything the client is
//! sent is built here from the layered rules (`data/rules/`) and the maps.
//!
//! Join sequence, as the 2016-08-06 client drives it (INFERRED from its
//! handlers; the original server is lost):
//!
//! 1. The player presses Play. With no room yet the client emits `create`,
//!    then always `respawn`.
//! 2. On `respawn` the server sends `welcome({id, room}, false)`. The
//!    client copies its current name and class into that object and
//!    answers `gotit(obj, flag, Date.now(), false)`.
//! 3. On `gotit` the server spawns the player and sends `gameSetup` (map
//!    the first time), then `add` to everyone else, then `lb` and `ts`.
//!
//! Because `respawn` carries nothing, the `welcome`/`gotit` round trip is
//! how a returning player's new name and class reach the server.

pub mod assumptions;
pub mod map;
pub mod maps;

use std::collections::HashMap;
use std::f64::consts::PI;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::eio::{ClientHandle, ConnId, TransportEvent};
use crate::sio::Event;
use crate::trace::Trace;
use assumptions::{Assumptions, Mode};
use map::{Body, Map};
use maps::MapSet;

/// Name shown for players who leave the name box empty (ASSUMPTION).
const DEFAULT_NAME: &str = "Guest";
/// Longest name the client itself allows (RECOVERED: `substring(0, 25)`).
const MAX_NAME: usize = 25;
/// The client waits this long after landing before it can jump again
/// (RECOVERED: `jumpCountdown = 250`).
const JUMP_COOLDOWN_MS: f64 = 250.0;

#[derive(Debug, Clone)]
struct Weapon {
    slot_spec: usize,
}

#[derive(Debug, Clone)]
struct Player {
    index: u32,
    id: u64,
    name: String,
    class_index: usize,
    team: String,
    x: f64,
    y: f64,
    angle: f64,
    width: f64,
    height: f64,
    speed: f64,
    jump_y: f64,
    jump_delta: f64,
    jump_strength: f64,
    gravity_strength: f64,
    jump_countdown: f64,
    name_y_offset: f64,
    dead: bool,
    health: f64,
    max_health: f64,
    score: u32,
    kills: u32,
    deaths: u32,
    spawn_protection_left: f64,
    weapons: Vec<Weapon>,
    current_weapon: usize,
    /// Last input sequence number applied (`isn`).
    isn: i64,
    /// Client timestamp of the last input, for its frame delta.
    last_input_ts: Option<f64>,
}

impl Player {
    fn body(&self) -> Body {
        Body {
            x: self.x,
            y: self.y,
            old_x: self.x,
            old_y: self.y,
            width: self.width,
            height: self.height,
            jump_y: self.jump_y,
        }
    }
}

struct Conn {
    handle: ClientHandle,
    host: String,
    room: Option<String>,
    /// Set by `respawn`, consumed by `gotit`.
    spawn_requested: bool,
    /// Whether this client already has the current map.
    has_map: bool,
    player: Option<u32>,
}

struct Room {
    name: String,
    mode: Mode,
    map: Map,
    players: HashMap<u32, Player>,
    next_index: u32,
    members: Vec<ConnId>,
}

/// The whole game state.
pub struct Game {
    rules: Assumptions,
    maps: MapSet,
    conns: HashMap<ConnId, Conn>,
    rooms: HashMap<String, Room>,
    trace: Trace,
    rng: u64,
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|f| f.is_finite())
}

fn event(name: &str, args: Vec<Value>) -> Event {
    Event::new(name, args)
}

/// Strips markup the way the client does before sending a name, and caps it.
fn clean_name(raw: &str) -> String {
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
    let trimmed: String = out.trim().chars().take(MAX_NAME).collect();
    if trimmed.is_empty() {
        DEFAULT_NAME.to_owned()
    } else {
        trimmed
    }
}

impl Game {
    #[must_use]
    pub fn new(rules: Assumptions, maps: MapSet, trace: Trace) -> Self {
        Self {
            rules,
            maps,
            conns: HashMap::new(),
            rooms: HashMap::new(),
            trace,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Runs until the transport channel closes.
    pub async fn run(mut self, mut events: mpsc::UnboundedReceiver<TransportEvent>) {
        let tick = Duration::from_secs_f64(1.0 / self.rules.net.update_hz);
        let mut interval = tokio::time::interval(tick);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = tokio::time::Instant::now();
        loop {
            tokio::select! {
                ev = events.recv() => match ev {
                    Some(ev) => self.handle(ev),
                    None => return,
                },
                now = interval.tick() => {
                    let dt = (now - last).as_secs_f64() * 1000.0;
                    last = now;
                    self.tick(dt);
                }
            }
        }
    }

    /// Applies one transport event.
    pub fn handle(&mut self, ev: TransportEvent) {
        match ev {
            TransportEvent::Connected { conn, handle, host } => {
                self.conns.insert(
                    conn,
                    Conn {
                        handle,
                        host,
                        room: None,
                        spawn_requested: false,
                        has_map: false,
                        player: None,
                    },
                );
            }
            TransportEvent::Event { conn, event } => self.on_event(conn, &event),
            TransportEvent::Disconnected { conn, .. } => self.leave(conn),
        }
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64*: spawn choice only, not security.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn emit(&self, conn: ConnId, ev: &Event) {
        if let Some(c) = self.conns.get(&conn) {
            c.handle.emit(ev);
        }
    }

    fn broadcast(&self, room: &str, ev: &Event, except: Option<ConnId>) {
        if let Some(r) = self.rooms.get(room) {
            for &m in &r.members {
                if Some(m) != except {
                    self.emit(m, ev);
                }
            }
        }
    }

    fn on_event(&mut self, conn: ConnId, ev: &Event) {
        if !self.conns.contains_key(&conn) {
            return;
        }
        let a = &ev.args;
        match ev.name.as_str() {
            "ping1" => self.emit(conn, &event("pong1", vec![])),
            "create" => {
                // With an argument this is a lobby join; private lobbies
                // are not built yet, so every create joins the public room.
                self.join_room(conn, "1");
            }
            "respawn" => self.on_respawn(conn),
            "gotit" => self.on_gotit(conn, a.first()),
            "4" => self.on_input(conn, a.first()),
            "0" => {
                if let Some(f) = num(a.first()) {
                    self.with_player(conn, |p| {
                        p.angle = ((f + 2.0 * PI) % (2.0 * PI)) * (180.0 / PI) + 90.0;
                    });
                }
            }
            "sw" => self.on_swap(conn, a.first()),
            "ftc" => self.on_fetch(conn, a.first()),
            _ => self.trace.note("unhandled", conn, &ev.name),
        }
    }

    fn join_room(&mut self, conn: ConnId, name: &str) {
        let Some(c) = self.conns.get(&conn) else {
            return;
        };
        if c.room.is_some() {
            return;
        }
        if !self.rooms.contains_key(name) {
            let mode = self
                .rules
                .mode()
                .cloned()
                .unwrap_or_else(|| self.rules.modes[0].clone());
            let r = self.next_random();
            let (map_id, map) = self.maps.pick(&mode, r);
            self.trace
                .note("room", conn, &format!("{name} {} map {map_id}", mode.code));
            self.rooms.insert(
                name.to_owned(),
                Room {
                    name: name.to_owned(),
                    mode,
                    map,
                    players: HashMap::new(),
                    next_index: 0,
                    members: Vec::new(),
                },
            );
        }
        let Some(c) = self.conns.get(&conn) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(name) else {
            return;
        };
        room.members.push(conn);
        let key = format!("{}/{}", c.host, room.name);
        let room_name = room.name.clone();
        if let Some(c) = self.conns.get_mut(&conn) {
            c.room = Some(room_name.clone());
        }
        self.emit(conn, &event("yourRoom", vec![json!(room_name), json!(key)]));
    }

    fn on_respawn(&mut self, conn: ConnId) {
        if self.conns.get(&conn).is_some_and(|c| c.room.is_none()) {
            self.join_room(conn, "1");
        }
        let Some(c) = self.conns.get_mut(&conn) else {
            return;
        };
        let Some(room) = c.room.clone() else { return };
        if let Some(index) = c.player
            && self
                .rooms
                .get(&room)
                .and_then(|r| r.players.get(&index))
                .is_some_and(|p| !p.dead)
        {
            // Already alive: ignore a duplicate respawn.
            return;
        }
        c.spawn_requested = true;
        let id = conn;
        self.emit(
            conn,
            &event(
                "welcome",
                vec![json!({"id": id, "room": room}), json!(false)],
            ),
        );
    }

    fn on_gotit(&mut self, conn: ConnId, obj: Option<&Value>) {
        let Some(c) = self.conns.get_mut(&conn) else {
            return;
        };
        if !std::mem::take(&mut c.spawn_requested) {
            return;
        }
        let Some(room_name) = c.room.clone() else {
            return;
        };
        let name = clean_name(
            obj.and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        let class_req = obj
            .and_then(|o| o.get("classIndex"))
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0);
        let first_map = !c.has_map;
        c.has_map = true;
        let existing = c.player;
        let Some(room) = self.rooms.get(&room_name) else {
            return;
        };
        let team = Self::pick_team(room, existing);
        let forced = room.mode.forced_class.clone();
        let spawn = self.pick_spawn(&room_name, team.as_deref());
        let rules = &self.rules;
        // Sniper War and Rocket War put everyone in one class, whatever
        // they picked; `you` in gameSetup tells the client.
        let class_req = forced
            .as_deref()
            .and_then(|f| rules.class_named(f))
            .unwrap_or(class_req);
        let (class_index, class) = rules.class(class_req);
        let Some(room) = self.rooms.get_mut(&room_name) else {
            return;
        };
        let index = existing.unwrap_or_else(|| {
            let i = room.next_index;
            room.next_index += 1;
            i
        });
        let prev = room.players.get(&index);
        let player = Player {
            index,
            id: conn,
            name,
            class_index,
            // Free for all: every player is their own team; "" would hide
            // them from the client's score table.
            team: team.unwrap_or_else(|| format!("p{index}")),
            x: spawn.0,
            y: spawn.1,
            angle: 0.0,
            width: class.width,
            height: class.height,
            speed: class.speed,
            jump_y: 0.0,
            jump_delta: 0.0,
            jump_strength: class.jump_strength,
            gravity_strength: class.gravity_strength,
            jump_countdown: 0.0,
            name_y_offset: 0.0,
            dead: false,
            health: class.max_health,
            max_health: class.max_health,
            score: prev.map_or(0, |p| p.score),
            kills: prev.map_or(0, |p| p.kills),
            deaths: prev.map_or(0, |p| p.deaths),
            spawn_protection_left: f64::from(
                u32::try_from(rules.player.spawn_protection_ms).unwrap_or(u32::MAX),
            ),
            weapons: class
                .weapons
                .iter()
                .map(|&w| Weapon { slot_spec: w })
                .collect(),
            current_weapon: 0,
            isn: prev.map_or(-1, |p| p.isn),
            last_input_ts: None,
        };
        room.players.insert(index, player);
        if let Some(c) = self.conns.get_mut(&conn) {
            c.player = Some(index);
        }
        self.send_game_setup(conn, &room_name, index, first_map);
        if let Some(room) = self.rooms.get(&room_name)
            && let Some(p) = room.players.get(&index)
        {
            let add = event("add", vec![json!(self.player_json(p).to_string())]);
            self.broadcast(&room_name, &add, Some(conn));
        }
        self.send_scores(&room_name);
    }

    /// The team a joining player plays on in a team mode: the one they
    /// had, else the smaller side, red on a tie (ASSUMED).
    fn pick_team(room: &Room, existing: Option<u32>) -> Option<String> {
        if !room.mode.teams {
            return None;
        }
        if let Some(p) = existing.and_then(|i| room.players.get(&i)) {
            return Some(p.team.clone());
        }
        let blue = room.players.values().filter(|p| p.team == "blue").count();
        let red = room.players.values().filter(|p| p.team == "red").count();
        Some(if blue < red { "blue" } else { "red" }.to_owned())
    }

    fn pick_spawn(&mut self, room: &str, team: Option<&str>) -> (f64, f64) {
        let r = self.next_random();
        let Some(room) = self.rooms.get(room) else {
            return (0.0, 0.0);
        };
        let tiles = room.map.spawn_tiles(team);
        if tiles.is_empty() {
            return (0.0, 0.0);
        }
        let t = tiles[usize::try_from(r % tiles.len() as u64).unwrap_or(0)];
        let s = room.map.scale;
        (t.x + s / 2.0, t.y + s / 2.0)
    }

    fn weapon_json(&self, w: &Weapon) -> Value {
        let spec = &self.rules.weapons[w.slot_spec];
        let mut v = serde_json::to_value(spec).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            // Per-player state the client keeps on the same object.
            m.insert("reloadTime".into(), json!(0));
            m.insert("spreadIndex".into(), json!(0));
            m.insert("lastShot".into(), json!(0));
            m.insert("front".into(), json!(true));
            // No camo: the client loads camos/<camo+1>.png for camo >= 0.
            m.insert("camo".into(), json!(-1));
        }
        v
    }

    fn player_json(&self, p: &Player) -> Value {
        let weapons: Vec<Value> = p.weapons.iter().map(|w| self.weapon_json(w)).collect();
        json!({
            "index": p.index,
            "id": p.id,
            "name": p.name,
            "classIndex": p.class_index,
            "team": p.team,
            "x": p.x.round(),
            "y": p.y.round(),
            "angle": p.angle.round(),
            "width": p.width,
            "height": p.height,
            "speed": p.speed,
            "jumpY": 0,
            "jumpDelta": 0,
            "jumpStrength": p.jump_strength,
            "gravityStrength": p.gravity_strength,
            "jumpCountdown": 0,
            "animIndex": 0,
            "frameCountdown": 0,
            "nameYOffset": p.name_y_offset,
            "dead": p.dead,
            "health": p.health,
            "maxHealth": p.max_health,
            "score": p.score,
            "kills": p.kills,
            "deaths": p.deaths,
            "likes": 0,
            "totalDamage": 0,
            "totalHealing": 0,
            "spawnProtection": i32::from(p.spawn_protection_left > 0.0),
            "weapons": weapons,
            "currentWeapon": p.current_weapon,
            "isn": p.isn,
            "loggedIn": false,
            "isBoss": false,
            "account": {"clan": "", "hat": null, "rank": 0},
        })
    }

    fn send_game_setup(&self, conn: ConnId, room_name: &str, index: u32, with_map: bool) {
        let Some(room) = self.rooms.get(room_name) else {
            return;
        };
        let Some(you) = room.players.get(&index) else {
            return;
        };
        let w = &self.rules.world;
        let (gw, gh) = room.map.world_size();
        let users: Vec<Value> = room
            .players
            .values()
            .filter(|p| p.index != index)
            .map(|p| self.player_json(p))
            .collect();
        let setup = json!({
            "mapData": {
                "genData": room.map.gen_data(),
                "width": gw,
                "height": gh,
                "gameMode": room.mode.client_json(),
                "clutter": [],
                "pickups": [],
            },
            "maxScreenWidth": w.max_screen_width,
            "maxScreenHeight": w.max_screen_height,
            "viewMult": w.view_mult,
            "tileScale": room.map.scale,
            "usersInRoom": users,
            "you": self.player_json(you),
        });
        self.emit(
            conn,
            &event(
                "gameSetup",
                vec![json!(setup.to_string()), json!(with_map), json!(true)],
            ),
        );
    }

    fn send_scores(&self, room_name: &str) {
        let Some(room) = self.rooms.get(room_name) else {
            return;
        };
        let mut order: Vec<&Player> = room.players.values().collect();
        order.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        let lb: Vec<u32> = order.iter().map(|p| p.index).collect();
        self.broadcast(room_name, &event("lb", vec![json!(lb)]), None);
        // RECOVERED (`updateTeamScores`): in free for all the first value
        // is the score limit the client divides your score by; in team
        // modes the two values are the red and blue bar widths in percent,
        // and the client shows your own team as "A". A team's score as the
        // sum of its players' scores is INFERRED.
        let ts = if room.mode.teams {
            let percent = |team: &str| {
                let total: u32 = room
                    .players
                    .values()
                    .filter(|p| p.team == team)
                    .map(|p| p.score)
                    .sum();
                (f64::from(total) * 100.0 / f64::from(room.mode.score.max(1))).min(100.0)
            };
            vec![
                json!(percent("red").round()),
                json!(percent("blue").round()),
            ]
        } else {
            vec![json!(room.mode.score), json!(0)]
        };
        self.broadcast(room_name, &event("ts", ts), None);
    }

    fn with_player<R>(&mut self, conn: ConnId, f: impl FnOnce(&mut Player) -> R) -> Option<R> {
        let c = self.conns.get(&conn)?;
        let index = c.player?;
        let room = c.room.as_ref()?;
        let p = self.rooms.get_mut(room)?.players.get_mut(&index)?;
        Some(f(p))
    }

    fn on_input(&mut self, conn: ConnId, input: Option<&Value>) {
        let Some(input) = input else { return };
        let (Some(hdt), Some(vdt), Some(ts)) = (
            num(input.get("hdt")),
            num(input.get("vdt")),
            num(input.get("ts")),
        ) else {
            return;
        };
        let isn = input.get("isn").and_then(Value::as_i64);
        let jump = num(input.get("s")).is_some_and(|s| s > 0.0);
        let max_dt = self.rules.net.max_input_delta_ms;
        let Some(c) = self.conns.get(&conn) else {
            return;
        };
        let (Some(index), Some(room_name)) = (c.player, c.room.clone()) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_name) else {
            return;
        };
        let Some(p) = room.players.get_mut(&index) else {
            return;
        };
        if let Some(isn) = isn {
            if isn <= p.isn {
                return;
            }
            p.isn = isn;
        }
        let delta = p
            .last_input_ts
            .map_or(0.0, |prev| (ts - prev).clamp(0.0, max_dt));
        p.last_input_ts = Some(ts);
        if p.dead {
            return;
        }
        // The client sends +-0.5 per axis and normalises the vector.
        let (dx, dy) = (hdt.clamp(-1.0, 1.0), vdt.clamp(-1.0, 1.0));
        let len = dx.hypot(dy);
        if len > 0.0 {
            let mut body = p.body();
            body.x += dx / len * p.speed * delta;
            body.y += dy / len * p.speed * delta;
            p.name_y_offset = room.map.collide(&mut body);
            p.x = body.x.round();
            p.y = body.y.round();
        }
        let mut jumped = false;
        if jump && p.jump_y <= 0.0 && p.jump_countdown <= 0.0 {
            p.jump_delta = p.jump_strength;
            p.jump_y = p.jump_delta;
            jumped = true;
        }
        if jumped {
            self.broadcast(&room_name, &event("jum", vec![json!(index)]), Some(conn));
        }
    }

    fn on_swap(&mut self, conn: ConnId, slot: Option<&Value>) {
        let Some(slot) = slot
            .and_then(Value::as_u64)
            .and_then(|s| usize::try_from(s).ok())
        else {
            return;
        };
        let Some(index) = self
            .with_player(conn, |p| {
                (slot < p.weapons.len() && !p.dead).then(|| {
                    p.current_weapon = slot;
                    p.index
                })
            })
            .flatten()
        else {
            return;
        };
        if let Some(room) = self.conns.get(&conn).and_then(|c| c.room.clone()) {
            self.broadcast(
                &room,
                &event("upd", vec![json!({"i": index, "wi": slot})]),
                Some(conn),
            );
        }
    }

    fn on_fetch(&mut self, conn: ConnId, index: Option<&Value>) {
        let Some(index) = index
            .and_then(Value::as_u64)
            .and_then(|i| u32::try_from(i).ok())
        else {
            return;
        };
        let Some(room) = self.conns.get(&conn).and_then(|c| c.room.clone()) else {
            return;
        };
        let json = self
            .rooms
            .get(&room)
            .and_then(|r| r.players.get(&index))
            .map(|p| self.player_json(p).to_string());
        if let Some(json) = json {
            self.emit(conn, &event("add", vec![json!(json)]));
        }
    }

    fn leave(&mut self, conn: ConnId) {
        let Some(c) = self.conns.remove(&conn) else {
            return;
        };
        let Some(room_name) = c.room else { return };
        let Some(room) = self.rooms.get_mut(&room_name) else {
            return;
        };
        room.members.retain(|&m| m != conn);
        if let Some(index) = c.player {
            room.players.remove(&index);
            self.broadcast(&room_name, &event("rem", vec![json!(index)]), None);
            self.send_scores(&room_name);
        }
        if self
            .rooms
            .get(&room_name)
            .is_some_and(|r| r.members.is_empty())
        {
            self.rooms.remove(&room_name);
        }
    }

    /// Advances time by `dt` milliseconds and sends snapshots.
    pub fn tick(&mut self, dt: f64) {
        let mut ended_protection = Vec::new();
        for room in self.rooms.values_mut() {
            for p in room.players.values_mut().filter(|p| !p.dead) {
                if p.spawn_protection_left > 0.0 {
                    p.spawn_protection_left -= dt;
                    if p.spawn_protection_left <= 0.0 {
                        ended_protection.push((room.name.clone(), p.index));
                    }
                }
                if p.jump_countdown > 0.0 {
                    p.jump_countdown -= dt;
                }
                if p.jump_y != 0.0 {
                    p.jump_delta -= p.gravity_strength * dt;
                    p.jump_y += p.jump_delta * dt;
                    if p.jump_y <= 0.0 {
                        p.jump_y = 0.0;
                        p.jump_delta = 0.0;
                        p.jump_countdown = JUMP_COOLDOWN_MS;
                    }
                }
            }
        }
        for (room, index) in ended_protection {
            self.broadcast(
                &room,
                &event("upd", vec![json!({"i": index, "sp": 0})]),
                None,
            );
        }
        self.send_snapshots();
    }

    fn send_snapshots(&self) {
        let w = &self.rules.world;
        let (half_w, half_h) = (
            w.max_screen_width * w.view_mult / 2.0,
            w.max_screen_height * w.view_mult / 2.0,
        );
        for room in self.rooms.values() {
            if room.players.values().all(|p| p.dead) {
                continue;
            }
            for &m in &room.members {
                // Only clients that have the map: one still in the menu
                // would ask (`ftc`) for every player it does not know yet.
                let Some(c) = self.conns.get(&m).filter(|c| c.has_map) else {
                    continue;
                };
                let me = c.player.and_then(|i| room.players.get(&i));
                let mut rsd: Vec<Value> = Vec::new();
                for p in room.players.values().filter(|p| !p.dead) {
                    let mine = Some(p.index) == c.player;
                    if !mine
                        && let Some(me) = me
                        // Only players in (or near) this client's view.
                        && ((p.x - me.x).abs() > half_w + p.width
                            || (p.y - me.y).abs() > half_h + p.height)
                    {
                        continue;
                    }
                    let last = if mine {
                        json!(p.isn)
                    } else {
                        json!(p.name_y_offset)
                    };
                    rsd.extend([
                        json!(6),
                        json!(p.index),
                        json!(p.x.round()),
                        json!(p.y.round()),
                        json!(p.angle.round()),
                        last,
                    ]);
                }
                if !rsd.is_empty() {
                    c.handle.emit(&event("rsd", vec![Value::Array(rsd)]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

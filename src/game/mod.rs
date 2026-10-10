//! The game server: rooms and the event contract of KRP's client.
//!
//! One task owns all game state and handles transport events and ticks in
//! order, so there are no locks in game logic. Each Socket.IO namespace is
//! a room (`/<room name>`), as in `KrunkerRevival` (KRP): the client asks
//! `/api/getIP?room=<name>` and then connects to `/<name>`.
//!
//! The rules themselves live in [`room`], a port of KRP's `server/room.ts`
//! and `server/game.ts`. This module only routes: it maps connections to
//! rooms and players, drives time, and delivers what rooms queue.
//!
//! The archived 2016 client joins through [`crate::classic`] instead: it
//! has no namespaces, so its `create` (or first `respawn`) seats it in the
//! configured classic room, and its events pass through
//! [`crate::classic::adapt`] on the way in and out.

pub mod admin;
pub mod assumptions;
pub mod data;
pub mod map;
pub mod maps;
pub mod projectile;
pub mod room;
mod tuning;

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::classic::{adapt, eio3};
use crate::eio::{ClientHandle, ConnId, TransportEvent};
use crate::sio::Event;
use crate::trace::Trace;
use assumptions::Assumptions;
use data::GameData;
use maps::MapSet;
use room::{Room, To};

/// A room the server opens at start: its name and starting mode code.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RoomSpec {
    pub name: String,
    pub mode: String,
    /// Player limit; `[game] max_players` when not given.
    #[serde(default)]
    pub max_players: Option<usize>,
}

/// A question the HTTP routes ask about the rooms.
#[derive(Debug)]
pub enum Ask {
    /// `/api/getRooms`.
    Rooms(oneshot::Sender<Value>),
    /// `/api/getIP?room=`: which room to join.
    Resolve(String, oneshot::Sender<Option<String>>),
    /// A command from the admin panel or console.
    Admin(crate::admin::Request),
}

const MAX_ROOM_NAME: usize = 24;
/// Highest player limit a room may be configured with.
pub const MAX_PLAYER_LIMIT: usize = 64;

/// How a player's client is reached.
enum Link {
    Krp(ClientHandle),
    Classic {
        handle: eio3::ClientHandle,
        host: String,
        member: adapt::Member,
    },
}

struct Slot {
    room: Room,
    members: HashMap<u32, Link>,
}

/// A 2016 client connection; `seat` is set once it joined a room.
struct ClassicConn {
    handle: eio3::ClientHandle,
    host: String,
    seat: Option<(String, u32)>,
}

pub struct Game {
    rules: Assumptions,
    data: GameData,
    maps: MapSet,
    rooms: Vec<Slot>,
    /// Connection to (room name, player index).
    conns: HashMap<ConnId, (String, u32)>,
    /// 2016 clients, keyed by their own Engine.IO 3 connection ids.
    classic: HashMap<eio3::ConnId, ClassicConn>,
    /// The room 2016 clients join.
    classic_room: String,
    trace: Trace,
    seed: u64,
    start: tokio::time::Instant,
    admin: admin::Admin,
}

fn valid_room_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ROOM_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Game {
    /// # Errors
    /// Fails if a configured room names an unknown mode or a bad name.
    pub fn new(
        rules: Assumptions,
        data: GameData,
        maps: MapSet,
        rooms: &[RoomSpec],
        trace: Trace,
    ) -> Result<Self, String> {
        let mut game = Self {
            rules,
            data,
            maps,
            rooms: Vec::new(),
            conns: HashMap::new(),
            classic: HashMap::new(),
            classic_room: String::new(),
            trace,
            seed: 0x9E37_79B9_7F4A_7C15,
            start: tokio::time::Instant::now(),
            admin: admin::Admin::default(),
        };
        for r in rooms {
            if !valid_room_name(&r.name) {
                return Err(format!(
                    "room name {:?}: letters, digits, - and _ only",
                    r.name
                ));
            }
            let mode = game
                .data
                .mode_index(&r.mode)
                .ok_or_else(|| format!("room {}: unknown mode {:?}", r.name, r.mode))?;
            if game.rooms.iter().any(|s| s.room.name == r.name) {
                return Err(format!("room {} is listed twice", r.name));
            }
            game.open_room(&r.name, mode);
            if let Some(n) = r.max_players {
                if !(1..=MAX_PLAYER_LIMIT).contains(&n) {
                    return Err(format!(
                        "room {}: max_players {n} is not between 1 and {MAX_PLAYER_LIMIT}",
                        r.name
                    ));
                }
                if let Some(s) = game.rooms.last_mut() {
                    s.room.set_player_limit(n);
                }
            }
        }
        if let Some(first) = game.rooms.first() {
            game.classic_room = first.room.name.clone();
        }
        Ok(game)
    }

    /// Sets the room 2016 clients join; the first room by default.
    ///
    /// # Errors
    /// Fails if no room has that name.
    pub fn set_classic_room(&mut self, name: &str) -> Result<(), String> {
        if !self.rooms.iter().any(|s| s.room.name == name) {
            return Err(format!("classic room {name} is not one of the rooms"));
        }
        name.clone_into(&mut self.classic_room);
        Ok(())
    }

    fn open_room(&mut self, name: &str, mode: usize) {
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let room = Room::new(
            name,
            &self.data,
            &self.maps,
            &self.rules.rules,
            &self.rules.world,
            mode,
            self.seed,
        );
        self.rooms.push(Slot {
            room,
            members: HashMap::new(),
        });
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// The room `/api/getIP` sends a client to: the named one, else the
    /// first (as KRP does).
    #[must_use]
    pub fn resolve_room(&self, name: &str) -> Option<String> {
        self.rooms
            .iter()
            .find(|s| s.room.name == name)
            .or_else(|| self.rooms.first())
            .map(|s| s.room.name.clone())
    }

    /// The room list for `/api/getRooms`.
    #[must_use]
    pub fn room_list(&self) -> Value {
        Value::Array(
            self.rooms
                .iter()
                .map(|s| s.room.list_entry(&self.data))
                .collect(),
        )
    }

    /// Answers one question from the HTTP side.
    pub fn answer(&mut self, ask: Ask) {
        match ask {
            Ask::Rooms(reply) => {
                let _ = reply.send(self.room_list());
            }
            Ask::Resolve(name, reply) => {
                let _ = reply.send(self.resolve_room(&name));
            }
            Ask::Admin(req) => self.admin(req),
        }
    }

    /// Runs until the transport channel closes, answering `queries` about
    /// the rooms and taking 2016 clients from `classic` in the same task.
    pub async fn run(
        mut self,
        mut events: mpsc::UnboundedReceiver<TransportEvent>,
        mut queries: mpsc::UnboundedReceiver<Ask>,
        mut classic: mpsc::UnboundedReceiver<eio3::TransportEvent>,
    ) {
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
                Some(q) = queries.recv() => self.answer(q),
                Some(ev) = classic.recv() => self.handle_classic(ev),
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
        let now = self.now();
        self.handle_at(ev, now);
    }

    fn handle_at(&mut self, ev: TransportEvent, now: f64) {
        match ev {
            TransportEvent::Connected {
                conn, handle, ns, ..
            } => self.connect(conn, handle, &ns),
            TransportEvent::Event { conn, event } => {
                let Some((name, index)) = self.conns.get(&conn).cloned() else {
                    return;
                };
                self.trace.event("in", conn, &event);
                let (data, maps, rules) = (&self.data, &self.maps, &self.rules.rules);
                if let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) {
                    s.room.on_event(data, maps, rules, index, &event, now);
                }
                self.deliver(&name);
            }
            TransportEvent::Disconnected { conn, .. } => {
                let Some((name, index)) = self.conns.remove(&conn) else {
                    return;
                };
                self.log_leave(&name, index);
                let (data, rules) = (&self.data, &self.rules.rules);
                if let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) {
                    s.members.remove(&index);
                    s.room.leave(data, rules, index, now);
                }
                self.deliver(&name);
            }
        }
    }

    fn log_leave(&self, room: &str, index: u32) {
        if self.admin.log.is_none() {
            return;
        }
        let who = self
            .rooms
            .iter()
            .find(|s| s.room.name == room)
            .and_then(|s| s.room.players.iter().find(|p| p.index == index))
            .map_or_else(String::new, |p| p.name.clone());
        self.log(room, "leave", format!("#{index} {who} left"));
    }

    /// Applies one transport event from a 2016 client.
    pub fn handle_classic(&mut self, ev: eio3::TransportEvent) {
        let now = self.now();
        self.handle_classic_at(ev, now);
    }

    fn handle_classic_at(&mut self, ev: eio3::TransportEvent, now: f64) {
        match ev {
            eio3::TransportEvent::Connected { conn, handle, host } => {
                self.classic.insert(
                    conn,
                    ClassicConn {
                        handle,
                        host,
                        seat: None,
                    },
                );
            }
            eio3::TransportEvent::Event { conn, event } => {
                self.trace.event("in", conn, &event);
                if matches!(event.name.as_str(), "create" | "respawn") {
                    self.seat_classic(conn);
                }
                let Some((name, index)) = self.classic.get(&conn).and_then(|c| c.seat.clone())
                else {
                    return;
                };
                let (data, maps, rules) = (&self.data, &self.maps, &self.rules.rules);
                let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) else {
                    return;
                };
                let Some(Link::Classic { member, .. }) = s.members.get_mut(&index) else {
                    return;
                };
                if let Some(event) = adapt::inbound(event, index, member) {
                    s.room.on_event(data, maps, rules, index, &event, now);
                }
                self.deliver(&name);
            }
            eio3::TransportEvent::Disconnected { conn, .. } => {
                let Some((name, index)) = self.classic.remove(&conn).and_then(|c| c.seat) else {
                    return;
                };
                self.log_leave(&name, index);
                let (data, rules) = (&self.data, &self.rules.rules);
                if let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) {
                    s.members.remove(&index);
                    s.room.leave(data, rules, index, now);
                }
                self.deliver(&name);
            }
        }
    }

    /// Seats a 2016 client in the classic room, unless it already has a
    /// seat. Lobbies are not built, so `create` with a lobby key joins the
    /// same room.
    fn seat_classic(&mut self, conn: eio3::ConnId) {
        let Some(c) = self.classic.get(&conn) else {
            return;
        };
        if c.seat.is_some() {
            return;
        }
        let name = self.classic_room.clone();
        let data = &self.data;
        let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) else {
            return;
        };
        if s.room.players.len() >= s.room.max_players {
            c.handle
                .emit(&Event::new("kick", vec![serde_json::json!("Room is full")]));
            return;
        }
        let index = s.room.join(data);
        // KRP's join opens the menu with `welcome(.., true)`; the 2016
        // client is already past its menu and asks with `respawn`.
        s.room
            .out
            .retain(|o| !(o.to == To::One(index) && o.event.name == "welcome"));
        s.members.insert(
            index,
            Link::Classic {
                handle: c.handle.clone(),
                host: c.host.clone(),
                member: adapt::Member::default(),
            },
        );
        if let Some(c) = self.classic.get_mut(&conn) {
            c.seat = Some((name.clone(), index));
        }
        self.trace.note("join", conn, &name);
        self.log(&name, "join", format!("#{index} joined (2016 client)"));
        self.deliver(&name);
    }

    fn connect(&mut self, conn: ConnId, handle: ClientHandle, ns: &str) {
        let name = ns.trim_start_matches('/');
        if name.is_empty() {
            // The root namespace carries no game; accept it so a client
            // that opens it is not refused.
            handle.accept();
            return;
        }
        let data = &self.data;
        let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) else {
            handle.refuse("Invalid namespace");
            return;
        };
        if s.room.players.len() >= s.room.max_players {
            handle.refuse("Room is full");
            return;
        }
        handle.accept();
        let index = s.room.join(data);
        s.members.insert(index, Link::Krp(handle));
        self.conns.insert(conn, (name.to_owned(), index));
        self.trace.note("join", conn, name);
        self.log(name, "join", format!("#{index} joined"));
        self.deliver(name);
    }

    /// Sends everything room `name` queued.
    fn deliver(&mut self, name: &str) {
        let data = &self.data;
        let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) else {
            return;
        };
        let out = std::mem::take(&mut s.room.out);
        let room = &s.room;
        let mut log = Vec::new();
        for o in out {
            if self.admin.log.is_some()
                && let Some(line) = admin_line(room, &o.event)
            {
                log.push(line);
            }
            let send = |link: &mut Link| send(link, &o.event, room, data);
            match &o.to {
                To::All => s.members.values_mut().for_each(send),
                To::One(i) => s.members.get_mut(i).into_iter().for_each(send),
                To::Team(t) => {
                    for p in room.players.iter().filter(|p| &p.team == t) {
                        s.members.get_mut(&p.index).into_iter().for_each(send);
                    }
                }
            }
            if !matches!(o.to, To::All) || o.event.name != "rsd" {
                self.trace.event("out", 0, &o.event);
            }
        }
        for (kind, text) in log {
            self.log(name, kind, text);
        }
    }

    /// Advances every room by `dt` milliseconds.
    pub fn tick(&mut self, dt: f64) {
        let now = self.now();
        self.tick_at(now, dt);
    }

    fn tick_at(&mut self, now: f64, dt: f64) {
        let names: Vec<String> = self.rooms.iter().map(|s| s.room.name.clone()).collect();
        for name in names {
            let (data, maps, rules) = (&self.data, &self.maps, &self.rules.rules);
            if self.admin.paused.contains(&name) {
                continue;
            }
            if let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) {
                s.room.tick(data, maps, rules, now, dt);
            }
            self.deliver(&name);
        }
    }
}

/// What the admin log shows for an outgoing event: the kill feed, chat
/// and round ends.
fn admin_line(room: &Room, event: &Event) -> Option<(&'static str, String)> {
    let a = &event.args;
    match event.name.as_str() {
        "5" => Some(("feed", a.first()?.as_str()?.to_owned())),
        "cht" => {
            let m = a.first()?.as_array()?;
            let text = m.get(1)?.as_str()?;
            let who = m.first()?.as_u64().and_then(|i| {
                room.players
                    .iter()
                    .find(|p| u64::from(p.index) == i)
                    .map(|p| format!("#{i} {}", p.name))
            });
            Some((
                "chat",
                format!("{}: {text}", who.as_deref().unwrap_or("server")),
            ))
        }
        "7" => {
            let w = a.first()?.as_str().unwrap_or_default();
            let w = if w.is_empty() { "nobody" } else { w };
            Some(("round", format!("round over, winner {w}")))
        }
        _ => None,
    }
}

fn send(link: &mut Link, event: &Event, room: &Room, data: &GameData) {
    match link {
        Link::Krp(h) => h.emit(event),
        Link::Classic {
            handle,
            host,
            member,
        } => {
            if let Some(e) = adapt::outbound(event, room, data, host, member) {
                handle.emit(&e);
            }
        }
    }
}

#[cfg(test)]
mod tests;

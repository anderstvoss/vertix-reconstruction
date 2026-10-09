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

pub mod assumptions;
pub mod data;
pub mod map;
pub mod maps;
pub mod projectile;
pub mod room;

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::eio::{ClientHandle, ConnId, TransportEvent};
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
}

/// A question the HTTP routes ask about the rooms.
#[derive(Debug)]
pub enum Ask {
    /// `/api/getRooms`.
    Rooms(oneshot::Sender<Value>),
    /// `/api/getIP?room=`: which room to join.
    Resolve(String, oneshot::Sender<Option<String>>),
}

const MAX_ROOM_NAME: usize = 24;

struct Slot {
    room: Room,
    members: HashMap<u32, ClientHandle>,
}

pub struct Game {
    rules: Assumptions,
    data: GameData,
    maps: MapSet,
    rooms: Vec<Slot>,
    /// Connection to (room name, player index).
    conns: HashMap<ConnId, (String, u32)>,
    trace: Trace,
    seed: u64,
    start: tokio::time::Instant,
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
            trace,
            seed: 0x9E37_79B9_7F4A_7C15,
            start: tokio::time::Instant::now(),
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
        }
        Ok(game)
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
    pub fn answer(&self, ask: Ask) {
        match ask {
            Ask::Rooms(reply) => {
                let _ = reply.send(self.room_list());
            }
            Ask::Resolve(name, reply) => {
                let _ = reply.send(self.resolve_room(&name));
            }
        }
    }

    /// Runs until the transport channel closes, answering `queries` about
    /// the rooms from the same task.
    pub async fn run(
        mut self,
        mut events: mpsc::UnboundedReceiver<TransportEvent>,
        mut queries: mpsc::UnboundedReceiver<Ask>,
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
                let (data, rules) = (&self.data, &self.rules.rules);
                if let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) {
                    s.members.remove(&index);
                    s.room.leave(data, rules, index, now);
                }
                self.deliver(&name);
            }
        }
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
        s.members.insert(index, handle);
        self.conns.insert(conn, (name.to_owned(), index));
        self.trace.note("join", conn, name);
        self.deliver(name);
    }

    /// Sends everything room `name` queued.
    fn deliver(&mut self, name: &str) {
        let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) else {
            return;
        };
        let out = std::mem::take(&mut s.room.out);
        for o in out {
            match &o.to {
                To::All => {
                    for h in s.members.values() {
                        h.emit(&o.event);
                    }
                }
                To::One(i) => {
                    if let Some(h) = s.members.get(i) {
                        h.emit(&o.event);
                    }
                }
                To::Team(t) => {
                    for p in s.room.players.iter().filter(|p| &p.team == t) {
                        if let Some(h) = s.members.get(&p.index) {
                            h.emit(&o.event);
                        }
                    }
                }
            }
            if !matches!(o.to, To::All) || o.event.name != "rsd" {
                self.trace.event("out", 0, &o.event);
            }
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
            if let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == name) {
                s.room.tick(data, maps, rules, now, dt);
            }
            self.deliver(&name);
        }
    }
}

#[cfg(test)]
mod tests;

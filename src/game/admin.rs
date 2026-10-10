//! Runs admin console commands (see [`crate::admin`]) inside the game task.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::assumptions::{Assumptions, Rules};
use super::data::GameData;
use super::{Game, Link, MAX_PLAYER_LIMIT, valid_room_name};
use crate::admin::{self, COMMANDS, Log, LogLine, Reply, Request, split_words};
use crate::sio::Event;

/// Files the console can reload from.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    pub krp_data: PathBuf,
    pub balance_dir: PathBuf,
    pub rules: Vec<PathBuf>,
}

/// The admin console's own state in the game.
#[derive(Debug, Default)]
pub struct Admin {
    pub log: Option<Log>,
    pub paths: Paths,
    /// Rooms whose server tick is stopped.
    pub paused: HashSet<String>,
}

type Res = Result<Reply, String>;

fn parse_num(s: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|f| f.is_finite())
        .ok_or_else(|| format!("{s:?} is not a number"))
}

fn parse_args(raw: Option<&String>) -> Result<Vec<Value>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    match serde_json::from_str::<Value>(raw).map_err(|e| format!("args: {e}"))? {
        Value::Array(a) => Ok(a),
        other => Ok(vec![other]),
    }
}

fn on_off(s: Option<&String>) -> Result<bool, String> {
    match s.map(String::as_str) {
        Some("on" | "true" | "1") => Ok(true),
        Some("off" | "false" | "0") => Ok(false),
        _ => Err("say on or off".into()),
    }
}

/// Commands that only read; everything else is logged.
const READ_ONLY: &[&str] = &[
    "help", "status", "state", "catalog", "players", "list", "use", "rooms",
];

impl Game {
    /// Connects the admin log and the files `reload` and `balance` read.
    pub fn set_admin(&mut self, log: Option<Log>, paths: Paths) {
        self.admin.log = log;
        self.admin.paths = paths;
    }

    /// Writes a line to the admin log, if anyone listens.
    pub(super) fn log(&self, room: &str, kind: &'static str, text: String) {
        if let Some(log) = &self.admin.log {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let t = self.now() as u64;
            let _ = log.send(LogLine {
                t,
                room: room.to_owned(),
                kind,
                text,
            });
        }
    }

    /// Runs one admin command and answers it.
    pub fn admin(&mut self, req: Request) {
        let reply = self.run_command(&req.line, req.room.as_deref());
        let _ = req.reply.send(reply);
    }

    /// Runs one command line in `room` (the first room if `None`).
    pub fn run_command(&mut self, line: &str, room: Option<&str>) -> Reply {
        let mut words = split_words(line);
        let mut room = room.map(str::to_owned);
        if let Some(at) = words.iter().position(|w| w.starts_with('@') && w.len() > 1) {
            room = Some(words.remove(at)[1..].to_owned());
        }
        if words.is_empty() {
            return Reply::err("empty command; `help` lists them");
        }
        let name = words[0].to_lowercase();
        let room = room
            .filter(|r| self.slot(r).is_some())
            .or_else(|| self.rooms.first().map(|s| s.room.name.clone()))
            .unwrap_or_default();
        let result = self.dispatch(&name, &words[1..], &room);
        let names: Vec<String> = self.rooms.iter().map(|s| s.room.name.clone()).collect();
        for n in names {
            self.deliver(&n);
        }
        let reply = result.unwrap_or_else(Reply::err);
        if reply.ok && !READ_ONLY.contains(&name.as_str()) {
            self.log(&room, "admin", format!("{line} -> {}", reply.text));
        }
        reply
    }

    fn slot(&self, name: &str) -> Option<&super::Slot> {
        self.rooms.iter().find(|s| s.room.name == name)
    }

    fn slot_mut(&mut self, name: &str) -> Result<&mut super::Slot, String> {
        self.rooms
            .iter_mut()
            .find(|s| s.room.name == name)
            .ok_or_else(|| format!("no room {name:?}"))
    }

    fn player_in(&self, room: &str, spec: Option<&String>) -> Result<u32, String> {
        let spec = spec.ok_or("which player?")?;
        self.slot(room)
            .ok_or_else(|| format!("no room {room:?}"))?
            .room
            .find_player(spec)
    }

    fn mode_from(&self, spec: &str) -> Result<usize, String> {
        let modes = &self.data.modes;
        if let Ok(i) = spec.parse::<usize>()
            && i < modes.len()
        {
            return Ok(i);
        }
        let low = spec.to_lowercase();
        modes
            .iter()
            .position(|m| m.code.to_lowercase() == low || m.name.to_lowercase() == low)
            .ok_or_else(|| {
                let codes: Vec<&str> = modes.iter().map(|m| m.code.as_str()).collect();
                format!("no mode {spec:?}; modes: {}", codes.join(" "))
            })
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch(&mut self, name: &str, a: &[String], room: &str) -> Res {
        let now = self.now();
        let arg = |i: usize| a.get(i);
        let rest = |from: usize| a.get(from..).map(|w| w.join(" ")).unwrap_or_default();
        match name {
            "help" | "?" => Ok(admin::help(arg(0).map(String::as_str))),
            "status" => Ok(Reply::ok(self.status_text()).with_data(self.state_json())),
            "state" => Ok(Reply::ok("state").with_data(self.state_json())),
            "catalog" => Ok(Reply::ok("catalog").with_data(self.catalog_json())),
            "rooms" => {
                let lines: Vec<String> = self
                    .rooms
                    .iter()
                    .map(|s| {
                        let r = &s.room;
                        format!(
                            "{} {} map {} players {}/{}{}",
                            r.name,
                            r.mode(&self.data).code,
                            r.map_id,
                            r.players.len(),
                            r.max_players,
                            if self.admin.paused.contains(&r.name) {
                                " (paused)"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect();
                Ok(Reply::ok(lines.join("\n")).with_data(self.room_list()))
            }
            "use" => {
                let r = arg(0).ok_or("which room?")?;
                self.slot(r).ok_or_else(|| format!("no room {r:?}"))?;
                Ok(Reply {
                    room: Some(r.clone()),
                    ..Reply::ok(format!("now working in {r}"))
                })
            }
            "players" => {
                let s = self.slot(room).ok_or("no rooms")?;
                let snap = s.room.admin_snapshot(&self.data);
                let lines: Vec<String> = s
                    .room
                    .players
                    .iter()
                    .map(|p| {
                        format!(
                            "#{} {:<16} team {:<5} {} hp {:.0}/{:.0} k {} d {} score {}{}",
                            p.index,
                            p.name,
                            p.team,
                            self.data
                                .classes
                                .get(p.class_index)
                                .and_then(|c| c.name.as_deref())
                                .unwrap_or("?"),
                            p.health,
                            p.max_health,
                            p.kills,
                            p.deaths,
                            p.score,
                            if p.dead { " (dead)" } else { "" }
                        )
                    })
                    .collect();
                let text = if lines.is_empty() {
                    format!("{room}: nobody here")
                } else {
                    format!("{room}:\n{}", lines.join("\n"))
                };
                Ok(Reply::ok(text).with_data(snap["players"].clone()))
            }
            "list" => self.list(arg(0).map_or("", String::as_str)),

            "mode" => {
                let mode = self.mode_from(arg(0).ok_or("which mode?")?)?;
                let map = match arg(1) {
                    Some(id) => Some(
                        self.maps
                            .get(id)
                            .map(|e| (e.id.clone(), e.map.clone()))
                            .ok_or_else(|| format!("no map {id:?}"))?,
                    ),
                    None => None,
                };
                self.new_round_in(room, mode, map, now)
            }
            "map" => {
                let id = arg(0).ok_or("which map?")?;
                let entry = self
                    .maps
                    .get(id)
                    .map(|e| (e.id.clone(), e.map.clone()))
                    .ok_or_else(|| format!("no map {id:?}; `list maps` lists them"))?;
                let mode = self.slot_mut(room)?.room.mode_index;
                self.new_round_in(room, mode, Some(entry), now)
            }
            "restart" => {
                let mode = self.slot_mut(room)?.room.mode_index;
                self.new_round_in(room, mode, None, now)
            }
            "win" | "lose" => {
                let who = arg(0).ok_or("which team or player?")?;
                let team = match who.as_str() {
                    "red" | "blue" => who.clone(),
                    "none" if name == "win" => String::new(),
                    spec => {
                        let i = self.player_in(room, Some(&spec.to_owned()))?;
                        let s = self.slot(room).ok_or("no room")?;
                        s.room
                            .players
                            .iter()
                            .find(|p| p.index == i)
                            .map(|p| p.team.clone())
                            .unwrap_or_default()
                    }
                };
                let (data, rules) = (&self.data, &self.rules.rules);
                let s = self
                    .rooms
                    .iter_mut()
                    .find(|s| s.room.name == room)
                    .ok_or("no room")?;
                let winner = if name == "lose" {
                    s.room.admin_winner_against(data, &team)
                } else {
                    team
                };
                s.room.admin_end_round(rules, &winner, now)?;
                let shown = if winner.is_empty() { "nobody" } else { &winner };
                Ok(Reply::ok(format!("{room}: round over, winner {shown}")))
            }
            "score" | "addscore" => {
                let who = arg(0).ok_or("which team or player?")?;
                let pts = parse_num(arg(1).ok_or("how many points?")?)?;
                let (data, rules) = (&self.data, &self.rules.rules);
                let s = self
                    .rooms
                    .iter_mut()
                    .find(|s| s.room.name == room)
                    .ok_or("no room")?;
                if who == "red" || who == "blue" {
                    let cur = if who == "red" {
                        s.room.score_red
                    } else {
                        s.room.score_blue
                    };
                    let add = if name == "score" { pts - cur } else { pts };
                    s.room.admin_add_team_score(data, rules, who, add, now)?;
                    return Ok(Reply::ok(format!("{room}: {who} score {}", cur + add)));
                }
                let i = s.room.find_player(who)?;
                let cur = s
                    .room
                    .players
                    .iter()
                    .find(|p| p.index == i)
                    .map_or(0.0, |p| p.score);
                let add = if name == "score" { pts - cur } else { pts };
                s.room.admin_add_score(data, rules, i, add, now);
                Ok(Reply::ok(format!("{room}: #{i} score {}", cur + add)))
            }
            "scorelimit" => {
                let v = arg(0).ok_or("how many points, or default?")?;
                let limit = if v == "default" {
                    None
                } else {
                    Some(parse_num(v)?.max(1.0))
                };
                let s = self.slot_mut(room)?;
                s.room.score_limit = limit;
                let shown = limit.map_or_else(|| "the mode's".to_owned(), |l| l.to_string());
                Ok(Reply::ok(format!("{room}: score limit {shown}")))
            }
            "pause" | "resume" => {
                self.slot_mut(room)?;
                if name == "pause" {
                    self.admin.paused.insert(room.to_owned());
                } else {
                    self.admin.paused.remove(room);
                }
                Ok(Reply::ok(format!("{room}: {name}d")))
            }

            "kick" => {
                let i = self.player_in(room, arg(0))?;
                let reason = Some(rest(1)).filter(|r| !r.is_empty());
                let reason = reason.unwrap_or_else(|| "Kicked by the server".into());
                self.kick(room, i, &reason, now);
                Ok(Reply::ok(format!("{room}: kicked #{i}")))
            }
            "kickall" => {
                let reason = Some(rest(0)).filter(|r| !r.is_empty());
                let reason = reason.unwrap_or_else(|| "Kicked by the server".into());
                let ids: Vec<u32> = self
                    .slot(room)
                    .map(|s| s.room.players.iter().map(|p| p.index).collect())
                    .unwrap_or_default();
                for &i in &ids {
                    self.kick(room, i, &reason, now);
                }
                Ok(Reply::ok(format!("{room}: kicked {}", ids.len())))
            }
            "kill" | "killall" => {
                let targets: Vec<u32> = if name == "kill" {
                    vec![self.player_in(room, arg(0))?]
                } else {
                    self.slot(room)
                        .map(|s| s.room.players.iter().map(|p| p.index).collect())
                        .unwrap_or_default()
                };
                let (data, rules) = (&self.data, &self.rules.rules);
                let s = self
                    .rooms
                    .iter_mut()
                    .find(|s| s.room.name == room)
                    .ok_or("no room")?;
                let n = targets
                    .into_iter()
                    .filter(|&i| s.room.admin_slay(data, rules, i, now))
                    .count();
                Ok(Reply::ok(format!("{room}: slew {n}")))
            }
            "health" => {
                let i = self.player_in(room, arg(0))?;
                let hp = parse_num(arg(1).ok_or("how much health?")?)?;
                let (data, rules) = (&self.data, &self.rules.rules);
                let s = self
                    .rooms
                    .iter_mut()
                    .find(|s| s.room.name == room)
                    .ok_or("no room")?;
                let hp = s.room.admin_set_health(data, rules, i, hp, now)?;
                Ok(Reply::ok(format!("{room}: #{i} health {hp}")))
            }
            "protect" => {
                let i = self.player_in(room, arg(0))?;
                let on = on_off(arg(1))?;
                self.slot_mut(room)?.room.admin_protect(i, on);
                Ok(Reply::ok(format!(
                    "{room}: #{i} protection {}",
                    if on { "on" } else { "off" }
                )))
            }
            "team" => {
                let i = self.player_in(room, arg(0))?;
                let team = arg(1).ok_or("which team?")?;
                let (data, rules) = (&self.data, &self.rules.rules);
                let s = self
                    .rooms
                    .iter_mut()
                    .find(|s| s.room.name == room)
                    .ok_or("no room")?;
                s.room.admin_set_team(data, rules, i, team, now)?;
                Ok(Reply::ok(format!("{room}: #{i} moved to {team}")))
            }
            "rename" => {
                let i = self.player_in(room, arg(0))?;
                let n = rest(1);
                let n: String = n
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(self.rules.rules.name_max_len)
                    .collect();
                if n.is_empty() {
                    return Err("what name?".into());
                }
                self.slot_mut(room)?.room.admin_rename(i, &n);
                Ok(Reply::ok(format!("{room}: #{i} is now {n}")))
            }
            "tp" => {
                let i = self.player_in(room, arg(0))?;
                let x = parse_num(arg(1).ok_or("x?")?)?;
                let y = parse_num(arg(2).ok_or("y?")?)?;
                self.slot_mut(room)?.room.admin_teleport(i, x, y);
                Ok(Reply::ok(format!("{room}: #{i} to {x},{y}")))
            }

            "maxplayers" => {
                let n = parse_num(arg(0).ok_or("how many?")?)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = n as usize;
                if !(1..=MAX_PLAYER_LIMIT).contains(&n) {
                    return Err(format!("between 1 and {MAX_PLAYER_LIMIT}"));
                }
                self.slot_mut(room)?.room.set_player_limit(n);
                Ok(Reply::ok(format!("{room}: max players {n}")))
            }
            "healthmult" | "speedmult" => {
                let x = parse_num(arg(0).ok_or("what multiplier?")?)?.clamp(0.01, 100.0);
                let r = &mut self.slot_mut(room)?.room;
                if name == "healthmult" {
                    r.admin_set_mults(Some(x), None);
                } else {
                    r.admin_set_mults(None, Some(x));
                }
                Ok(Reply::ok(format!("{room}: {name} {x} from next spawn")))
            }
            "pickups" => {
                let n = self.slot_mut(room)?.room.admin_refill_pickups();
                Ok(Reply::ok(format!("{room}: {n} pickups back")))
            }
            "say" => {
                let text = rest(0);
                if text.is_empty() {
                    return Err("say what?".into());
                }
                self.slot_mut(room)?.room.admin_say(&text);
                Ok(Reply::ok(format!("{room}: said it")))
            }
            "announce" => {
                let title = arg(0).ok_or("what title?")?.clone();
                let text = rest(1);
                self.slot_mut(room)?.room.admin_announce(&title, &text);
                Ok(Reply::ok(format!("{room}: announced")))
            }
            "sync" => {
                self.slot_mut(room)?.room.admin_sync();
                Ok(Reply::ok(format!("{room}: synced")))
            }

            "open" => {
                let r = arg(0).ok_or("room name?")?;
                let mode = self.mode_from(arg(1).ok_or("which mode?")?)?;
                if !valid_room_name(r) {
                    return Err("room names: letters, digits, - and _ only".into());
                }
                if self.slot(r).is_some() {
                    return Err(format!("{r} is already open"));
                }
                self.open_room(r, mode);
                Ok(Reply::ok(format!("opened {r}")))
            }
            "close" => {
                let r = arg(0).ok_or("which room?")?.clone();
                self.slot_mut(&r)?;
                if self.rooms.len() == 1 {
                    return Err("that is the last room".into());
                }
                let ids: Vec<u32> = self
                    .slot(&r)
                    .map(|s| s.room.players.iter().map(|p| p.index).collect())
                    .unwrap_or_default();
                for i in ids {
                    self.kick(&r, i, "The room was closed", now);
                }
                self.deliver(&r);
                self.rooms.retain(|s| s.room.name != r);
                self.admin.paused.remove(&r);
                if self.classic_room == r
                    && let Some(first) = self.rooms.first()
                {
                    self.classic_room = first.room.name.clone();
                }
                Ok(Reply::ok(format!("closed {r}")))
            }
            "classic" => {
                let r = arg(0).ok_or("which room?")?;
                self.set_classic_room(r)?;
                Ok(Reply::ok(format!("2016 clients now join {r}")))
            }
            "rule" => self.rule(arg(0), arg(1)),
            "reload" => {
                if arg(0).map(String::as_str) != Some("rules") {
                    return Err("reload what? (rules)".into());
                }
                let paths = self.admin.paths.rules.clone();
                if paths.is_empty() {
                    return Err("no rule layers configured".into());
                }
                let (rules, prov) = Assumptions::load_layers(&paths).map_err(|e| e.to_string())?;
                self.rules = rules;
                Ok(Reply::ok(format!("rules reloaded: {}", prov.summary())))
            }
            "balance" => {
                let Some(id) = arg(0) else {
                    return Ok(Reply::ok(format!(
                        "balance preset {} ({})",
                        self.data.balance.id, self.data.balance.title
                    )));
                };
                let p = &self.admin.paths;
                let data =
                    GameData::load(&p.krp_data, &p.balance_dir, id).map_err(|e| e.to_string())?;
                if data.modes.len() != self.data.modes.len()
                    || data.classes.len() != self.data.classes.len()
                    || data.weapons.len() != self.data.weapons.len()
                {
                    return Err(
                        "that preset changes the tables' shape; restart the server with it".into(),
                    );
                }
                self.data = data;
                Ok(Reply::ok(format!(
                    "balance preset {} from each player's next spawn",
                    self.data.balance.id
                )))
            }

            "emit" => {
                let who = arg(0).ok_or("to which player, or all?")?;
                let event = arg(1).ok_or("which event?")?.clone();
                let args = parse_args(arg(2))?;
                let to = if who == "all" {
                    None
                } else {
                    Some(self.player_in(room, Some(who))?)
                };
                self.slot_mut(room)?.room.admin_emit(to, &event, args);
                Ok(Reply::ok(format!("{room}: sent {event}")))
            }
            "inject" => {
                let i = self.player_in(room, arg(0))?;
                let event = Event::new(arg(1).ok_or("which event?")?, parse_args(arg(2))?);
                let (data, maps, rules) = (&self.data, &self.maps, &self.rules.rules);
                let s = self
                    .rooms
                    .iter_mut()
                    .find(|s| s.room.name == room)
                    .ok_or("no room")?;
                s.room.on_event(data, maps, rules, i, &event, now);
                Ok(Reply::ok(format!("{room}: #{i} sent {}", event.name)))
            }
            other => Err(format!("no command {other:?}; `help` lists them")),
        }
    }

    fn new_round_in(
        &mut self,
        room: &str,
        mode: usize,
        map: Option<(String, super::map::Map)>,
        now: f64,
    ) -> Res {
        let (data, maps, rules) = (&self.data, &self.maps, &self.rules.rules);
        let s = self
            .rooms
            .iter_mut()
            .find(|s| s.room.name == room)
            .ok_or_else(|| format!("no room {room:?}"))?;
        s.room.admin_new_round(data, maps, rules, mode, map, now);
        Ok(Reply::ok(format!(
            "{room}: new round, {} on map {}",
            s.room.mode(data).code,
            s.room.map_id
        )))
    }

    /// Disconnects a player with a reason the client shows.
    pub(super) fn kick(&mut self, room: &str, index: u32, reason: &str, now: f64) {
        let (data, rules) = (&self.data, &self.rules.rules);
        let Some(s) = self.rooms.iter_mut().find(|s| s.room.name == room) else {
            return;
        };
        let kick = Event::new("kick", vec![json!(reason)]);
        match s.members.remove(&index) {
            Some(Link::Krp(h)) => {
                h.emit(&kick);
                h.close();
            }
            Some(Link::Classic { handle, .. }) => {
                handle.emit(&kick);
                handle.close();
            }
            None => {}
        }
        s.room.leave(data, rules, index, now);
        let seat = (room.to_owned(), index);
        self.conns.retain(|_, v| *v != seat);
        for c in self.classic.values_mut() {
            if c.seat.as_ref() == Some(&seat) {
                c.seat = None;
            }
        }
        self.log(room, "leave", format!("#{index} kicked: {reason}"));
        self.deliver(room);
    }

    fn rule(&mut self, name: Option<&String>, value: Option<&String>) -> Res {
        let current = serde_json::to_value(&self.rules.rules).map_err(|e| e.to_string())?;
        let Some(name) = name else {
            let lines: Vec<String> = current
                .as_object()
                .map(|o| o.iter().map(|(k, v)| format!("{k} = {v}")).collect())
                .unwrap_or_default();
            return Ok(Reply::ok(lines.join("\n")).with_data(current));
        };
        let Some(old) = current.get(name.as_str()) else {
            return Err(format!("no rule {name:?}; `rule` lists them"));
        };
        let Some(value) = value else {
            return Ok(Reply::ok(format!("{name} = {old}")));
        };
        let new: Value =
            serde_json::from_str(value).map_err(|_| format!("{value:?} is not a number"))?;
        let mut next = current.clone();
        next[name.as_str()] = new;
        let rules: Rules = serde_json::from_value(next).map_err(|e| format!("{name}: {e}"))?;
        self.rules.rules = rules;
        Ok(Reply::ok(format!("{name} = {value} (was {old})")))
    }

    fn list(&self, what: &str) -> Res {
        let d = &self.data;
        let (lines, data): (Vec<String>, Value) =
            match what {
                "modes" => (
                    d.modes
                        .iter()
                        .enumerate()
                        .map(|(i, m)| {
                            format!(
                                "{i} {} {} (score {}, maps {})",
                                m.code,
                                m.name,
                                m.score,
                                m.maps.join(",")
                            )
                        })
                        .collect(),
                    self.catalog_json()["modes"].clone(),
                ),
                "maps" => (
                    self.maps.ids().iter().map(|s| (*s).to_owned()).collect(),
                    json!(self.maps.ids()),
                ),
                "classes" => (
                    d.classes
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            format!(
                                "{i} {} hp {} speed {}{}",
                                c.name.as_deref().unwrap_or("?"),
                                c.max_health,
                                c.speed,
                                if c.available { "" } else { " (hidden)" }
                            )
                        })
                        .collect(),
                    self.catalog_json()["classes"].clone(),
                ),
                "weapons" => (
                    d.weapons
                        .iter()
                        .enumerate()
                        .map(|(i, w)| {
                            format!(
                                "{i} {} dmg {} reload {}",
                                w.spec.name, w.spec.dmg, w.spec.reload_speed
                            )
                        })
                        .collect(),
                    self.catalog_json()["weapons"].clone(),
                ),
                "hats" | "shirts" | "camos" | "sprays" => {
                    let c = &d.cosmetics;
                    let list = match what {
                        "hats" => &c.hats,
                        "shirts" => &c.shirts,
                        "camos" => &c.camos,
                        _ => &c.sprays,
                    };
                    (
                        list.iter()
                            .map(|v| {
                                format!(
                                    "{} {}",
                                    v["id"],
                                    v.get("name").and_then(Value::as_str).unwrap_or("")
                                )
                            })
                            .collect(),
                        json!(list),
                    )
                }
                "presets" => {
                    let p = self.presets();
                    (p.clone(), json!(p))
                }
                _ => return Err(
                    "list modes, maps, classes, weapons, hats, shirts, camos, sprays or presets"
                        .into(),
                ),
            };
        Ok(Reply::ok(lines.join("\n")).with_data(data))
    }

    /// Balance presets in the balance directory.
    fn presets(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(&self.admin.paths.balance_dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter_map(|n| n.strip_suffix(".json").map(str::to_owned))
                    .filter(|n| n != "index")
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    fn status_text(&self) -> String {
        let mut out = format!("balance {}\n", self.data.balance.id);
        for s in &self.rooms {
            let r = &s.room;
            let mode = r.mode(&self.data);
            let _ = write!(
                out,
                "{}: {} on map {}, round {}{}{}, {}/{} players",
                r.name,
                mode.code,
                r.map_id,
                if r.round_end { "ending" } else { "running" },
                if mode.teams {
                    format!(" red {} blue {}", r.score_red, r.score_blue)
                } else {
                    String::new()
                },
                if self.admin.paused.contains(&r.name) {
                    " (paused)"
                } else {
                    ""
                },
                r.players.len(),
                r.max_players,
            );
            for p in &r.players {
                let _ = write!(
                    out,
                    "\n  #{} {} ({}) score {}",
                    p.index, p.name, p.team, p.score
                );
            }
            out.push('\n');
        }
        out.trim_end().to_owned()
    }

    /// Everything that changes, for the admin panel.
    #[must_use]
    pub fn state_json(&self) -> Value {
        let rooms: Vec<Value> = self
            .rooms
            .iter()
            .map(|s| {
                let mut v = s.room.admin_snapshot(&self.data);
                v["paused"] = json!(self.admin.paused.contains(&s.room.name));
                let links: serde_json::Map<String, Value> = s
                    .members
                    .iter()
                    .map(|(i, l)| {
                        let kind = match l {
                            Link::Krp(_) => "krp",
                            Link::Classic { .. } => "2016",
                        };
                        (i.to_string(), json!(kind))
                    })
                    .collect();
                if let Some(ps) = v["players"].as_array_mut() {
                    for p in ps {
                        let key = p["index"].to_string();
                        p["client"] = links.get(&key).cloned().unwrap_or(Value::Null);
                    }
                }
                v
            })
            .collect();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let uptime = self.now() as u64;
        json!({
            "uptimeMs": uptime,
            "balance": self.data.balance.id,
            "classicRoom": self.classic_room,
            "rules": serde_json::to_value(&self.rules.rules).unwrap_or(Value::Null),
            "rooms": rooms,
        })
    }

    /// What rarely changes: modes, maps, classes, weapons, presets.
    #[must_use]
    pub fn catalog_json(&self) -> Value {
        let d = &self.data;
        json!({
            "commands": COMMANDS,
            "modes": d.modes.iter().enumerate().map(|(i, m)| json!({
                "index": i, "code": m.code, "name": m.name, "teams": m.teams,
                "score": m.score, "maps": m.maps,
            })).collect::<Vec<_>>(),
            "maps": self.maps.ids(),
            "classes": d.classes.iter().enumerate().map(|(i, c)| json!({
                "index": i, "name": c.name, "available": c.available,
                "maxHealth": c.max_health, "speed": c.speed,
            })).collect::<Vec<_>>(),
            "weapons": d.weapons.iter().enumerate().map(|(i, w)| json!({
                "index": i, "name": w.spec.name, "dmg": w.spec.dmg,
                "reload": w.spec.reload_speed,
            })).collect::<Vec<_>>(),
            "presets": self.presets(),
        })
    }
}

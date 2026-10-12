//! What the admin console can do to one room (see [`crate::admin`]).
//!
//! Everything here is an addition for testing and hosting; none of it is
//! in KRP. Each action reuses the events the client already understands,
//! so a forced round end looks like a normal one, a slain player like a
//! suicide, and so on.

use serde_json::{Value, json};

use super::{Player, Room, Timer, To};
use crate::game::assumptions::Rules;
use crate::game::data::GameData;
use crate::game::map::Map;
use crate::game::maps::MapSet;

impl Room {
    /// Ends the round now, as reaching the score limit does: the stat
    /// table and mode vote open and the countdown to the next round runs.
    /// `winner` is a team (`red`, `blue`, or in free-for-all modes a
    /// player's index), or empty for no winner.
    ///
    /// # Errors
    /// Fails if the round is already over.
    pub fn admin_end_round(&mut self, rules: &Rules, winner: &str, now: f64) -> Result<(), String> {
        if self.round_end {
            return Err(format!("{}: the round is already over", self.name));
        }
        self.round_end = true;
        let votes = self.votes_json();
        self.send(To::All, "7", vec![json!(winner), votes, json!(false)]);
        self.timers.push((
            now + 1000.0,
            Timer::Countdown {
                left: i64::from(rules.round_end_countdown_s),
            },
        ));
        Ok(())
    }

    /// Starts a new round at once in `mode`, on `map` if given (else a
    /// random map of the mode), and sends every player to the menu as the
    /// end of a round's countdown does.
    pub fn admin_new_round(
        &mut self,
        data: &GameData,
        maps: &MapSet,
        rules: &Rules,
        mode: usize,
        map: Option<(String, Map)>,
        now: f64,
    ) {
        match map {
            Some((id, m)) => {
                self.new_round(data, maps, rules, mode, Some(m), now);
                self.map_id = id;
            }
            None => self.new_round(data, maps, rules, mode, None, now),
        }
        let ids: Vec<u32> = self.players.iter().map(|p| p.index).collect();
        for i in ids {
            let w = self.welcome_json(i);
            self.send(To::One(i), "welcome", vec![w, json!(true)]);
        }
    }

    /// Kills a player as a suicide that scores nothing for anyone.
    /// Returns false if the player is not alive.
    pub fn admin_slay(&mut self, data: &GameData, rules: &Rules, index: u32, now: f64) -> bool {
        let Some(p) = self.player_mut(index) else {
            return false;
        };
        if p.dead {
            return false;
        }
        let delta = -p.health;
        p.health = 0.0;
        // All damage from themselves: no assists, and the suicide's own
        // score is 0 (the kill's base health is used up).
        p.damage_sources.clear();
        p.damage_sources.insert(index, p.max_health);
        self.send(
            To::All,
            "1",
            vec![json!({
                "dID": index,
                "gID": index,
                "dir": 0,
                "healthDelta": delta,
                "bulletIndex": null,
                "health": 0,
            })],
        );
        self.kill(data, rules, index, index, now);
        true
    }

    /// Sets a player's health; 0 or less slays them.
    ///
    /// # Errors
    /// Fails if the player is missing or dead.
    pub fn admin_set_health(
        &mut self,
        data: &GameData,
        rules: &Rules,
        index: u32,
        health: f64,
        now: f64,
    ) -> Result<f64, String> {
        let p = self
            .player_mut(index)
            .ok_or_else(|| format!("no player {index}"))?;
        if p.dead {
            return Err(format!("{} is dead", p.name));
        }
        if health <= 0.0 {
            self.admin_slay(data, rules, index, now);
            return Ok(0.0);
        }
        let delta = health - p.health;
        p.health = health;
        self.send(
            To::All,
            "1",
            vec![json!({"gID": index, "healthDelta": delta, "health": health})],
        );
        Ok(health)
    }

    /// Changes a player's score by `points`, through the normal scoring
    /// path: boards update and the round ends if the limit is reached.
    pub fn admin_add_score(
        &mut self,
        data: &GameData,
        rules: &Rules,
        index: u32,
        points: f64,
        now: f64,
    ) {
        self.update_score(data, rules, points, index, now);
    }

    /// Changes a team's score by `points`, ending the round if it reaches
    /// the limit.
    ///
    /// # Errors
    /// Fails if `team` is not red or blue.
    pub fn admin_add_team_score(
        &mut self,
        data: &GameData,
        rules: &Rules,
        team: &str,
        points: f64,
        now: f64,
    ) -> Result<(), String> {
        match team {
            "red" => self.score_red += points,
            "blue" => self.score_blue += points,
            _ => return Err(format!("team must be red or blue, not {team:?}")),
        }
        self.send(
            To::All,
            "ts",
            vec![json!(self.score_red), json!(self.score_blue)],
        );
        let limit = self.score_limit.unwrap_or(self.mode(data).score).max(1.0);
        let lead = (self.score_red.max(self.score_blue) * 100.0 / limit).min(100.0);
        self.score_lb = lead.round();
        let reached = if team == "red" {
            self.score_red
        } else {
            self.score_blue
        } >= limit;
        if reached && !self.round_end {
            self.admin_end_round(rules, team, now)?;
        }
        Ok(())
    }

    /// Health and speed multipliers, as the custom server form sets them.
    /// They apply from each player's next spawn.
    pub fn admin_set_mults(&mut self, health: Option<f64>, speed: Option<f64>) {
        if let Some(h) = health {
            self.mults_health = h;
        }
        if let Some(s) = speed {
            self.mults_speed = s;
        }
    }

    #[must_use]
    pub fn admin_mults(&self) -> (f64, f64) {
        (self.mults_health, self.mults_speed)
    }

    /// Turns a player's spawn protection on or off until their next spawn.
    pub fn admin_protect(&mut self, index: u32, on: bool) -> bool {
        let Some(p) = self.player_mut(index) else {
            return false;
        };
        p.spawn_protected = on;
        // A pending end-of-protection timer belongs to this life; a new
        // life number makes it stale.
        p.life += 1;
        self.send(To::All, "upd", vec![json!({"i": index, "sp": on})]);
        true
    }

    /// Moves a player to a team. They are slain so they respawn on it.
    ///
    /// # Errors
    /// Fails if the mode has no teams, or on an unknown team or player.
    pub fn admin_set_team(
        &mut self,
        data: &GameData,
        rules: &Rules,
        index: u32,
        team: &str,
        now: f64,
    ) -> Result<(), String> {
        if !self.mode(data).teams {
            return Err(format!("{}: the mode has no teams", self.name));
        }
        if team != "red" && team != "blue" {
            return Err(format!("team must be red or blue, not {team:?}"));
        }
        let p = self
            .player_mut(index)
            .ok_or_else(|| format!("no player {index}"))?;
        team.clone_into(&mut p.team);
        self.admin_slay(data, rules, index, now);
        let Some(p) = self.player(index) else {
            return Ok(());
        };
        let add = self.player_json(p).to_string();
        self.send(To::All, "add", vec![json!(add)]);
        Ok(())
    }

    /// Renames a player; other clients see it from the player's next spawn.
    pub fn admin_rename(&mut self, index: u32, name: &str) -> bool {
        let Some(p) = self.player_mut(index) else {
            return false;
        };
        name.clone_into(&mut p.name);
        true
    }

    /// Moves a player, with Zone War's teleport event (the client shows
    /// a smoke puff and its "zone entered" text).
    pub fn admin_teleport(&mut self, index: u32, x: f64, y: f64) -> bool {
        let Some(p) = self.player_mut(index) else {
            return false;
        };
        p.x = x;
        p.y = y;
        self.send(
            To::All,
            "tprt",
            vec![json!({"indx": index, "score": 0, "newX": x, "newY": y})],
        );
        true
    }

    /// Makes every pickup (health packs, loot crates) available now.
    pub fn admin_refill_pickups(&mut self) -> usize {
        let mut n = 0;
        for i in 0..self.world.pickups.len() {
            if self.world.pickups[i].active {
                continue;
            }
            self.world.pickups[i].active = true;
            n += 1;
            let v = serde_json::to_value(&self.world.pickups[i]).unwrap_or(Value::Null);
            self.send(To::All, "4", vec![v, json!(i), json!(0)]);
        }
        n
    }

    /// Sends a chat line from the server (shown as a system message).
    pub fn admin_say(&mut self, text: &str) {
        self.send(To::All, "cht", vec![json!([-1, text])]);
    }

    /// Shows the big centre text to every living player.
    pub fn admin_announce(&mut self, title: &str, text: &str) {
        self.send(To::All, "6", vec![json!(title), json!(text), json!(1.25)]);
    }

    /// Sends everyone the positions of every player.
    pub fn admin_sync(&mut self) {
        let rsd = self.rsd(None);
        self.send(To::All, "rsd", vec![rsd]);
    }

    /// Queues any event for one player or everyone.
    pub fn admin_emit(&mut self, to: Option<u32>, name: &str, args: Vec<Value>) {
        self.send(to.map_or(To::All, To::One), name, args);
    }

    /// Finds a player by index, exact name, or unique name prefix
    /// (ignoring case).
    ///
    /// # Errors
    /// Fails if nobody or more than one player matches.
    pub fn find_player(&self, spec: &str) -> Result<u32, String> {
        if let Ok(i) = spec.trim_start_matches('#').parse::<u32>()
            && self.player(i).is_some()
        {
            return Ok(i);
        }
        let low = spec.to_lowercase();
        if let Some(p) = self.players.iter().find(|p| p.name.to_lowercase() == low) {
            return Ok(p.index);
        }
        let hits: Vec<&Player> = self
            .players
            .iter()
            .filter(|p| p.name.to_lowercase().starts_with(&low))
            .collect();
        match hits.as_slice() {
            [p] => Ok(p.index),
            [] => Err(format!("{}: no player {spec:?}", self.name)),
            _ => Err(format!(
                "{}: {spec:?} matches {} players; use the index",
                self.name,
                hits.len()
            )),
        }
    }

    /// The winner for "make `loser` lose": the other team, or in
    /// free-for-all modes the best-scoring other player (empty if none).
    #[must_use]
    pub fn admin_winner_against(&self, data: &GameData, loser: &str) -> String {
        if self.mode(data).teams {
            return if loser == "red" { "blue" } else { "red" }.to_owned();
        }
        self.players
            .iter()
            .filter(|p| p.team != loser)
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .map(|p| p.team.clone())
            .unwrap_or_default()
    }

    /// The room's state for the admin panel.
    #[must_use]
    pub fn admin_snapshot(&self, data: &GameData) -> Value {
        let mode = self.mode(data);
        let players: Vec<Value> = self
            .players
            .iter()
            .map(|p| {
                json!({
                    "index": p.index,
                    "name": p.name,
                    "team": p.team,
                    "classIndex": p.class_index,
                    "class": data.classes.get(p.class_index).and_then(|c| c.name.clone()),
                    "health": p.health,
                    "maxHealth": p.max_health,
                    "kills": p.kills,
                    "deaths": p.deaths,
                    "score": p.score,
                    "dead": p.dead,
                    "protected": p.spawn_protected,
                    "boss": p.is_boss,
                    "x": p.x.round(),
                    "y": p.y.round(),
                })
            })
            .collect();
        json!({
            "name": self.name,
            "mode": mode.code,
            "modeIndex": self.mode_index,
            "modeName": mode.name,
            "teams": mode.teams,
            "map": self.map_id,
            "round": self.round,
            "roundEnd": self.round_end,
            "scoreRed": self.score_red,
            "scoreBlue": self.score_blue,
            "scoreLimit": self.score_limit.unwrap_or(mode.score),
            "scoreLimitOverride": self.score_limit,
            "leadPercent": self.score_lb,
            "maxPlayers": self.max_players,
            "playerLimit": self.player_limit,
            "healthMult": self.mults_health,
            "speedMult": self.mults_speed,
            "votes": self.votes_json(),
            "bulletsActive": self.bullets.iter().filter(|b| b.active).count(),
            "players": players,
        })
    }
}

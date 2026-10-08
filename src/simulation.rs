//! Deterministic, headless NEW FFA model for client interoperability testing.
//!
//! The original Vertix server code is lost. All rules and default values in
//! this file are NEW reconstruction hypotheses and are NOT historic facts.
//! Recovered client wire contract: 2016-08-06 event "4" contains hdt, vdt,
//! ts, isn, s; "rsd" acknowledges isn; the client emits event "1" with six
//! positional arguments. Transport adapters belong outside this module.

use std::collections::BTreeMap;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceStatus {
    /// Confirmed from a specific archived original source, not used as defaults.
    Recovered,
    /// Taken provisionally from later/community observations.
    Provisional,
    /// Explicit reconstruction rule that has no historical claim.
    New,
    /// No rule/value available: must not silently become a numeric default.
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    pub status: EvidenceStatus,
    pub tick_ms: u64,
    pub world_width: f64,
    pub world_height: f64,
    pub move_units_per_ms: f64,
    pub projectile_units_per_ms: f64,
    pub projectile_lifetime_ms: u64,
    pub hit_radius: f64,
    pub max_health: i32,
    pub damage_per_hit: i32,
    pub fire_cooldown_ms: u64,
    pub reload_ms: u64,
    pub max_ammo: u16,
    pub respawn_ms: u64,
    pub score_limit: u32,
}

impl Rules {
    /// Every value is a NEW test fixture, never a recovered original server stat.
    #[must_use]
    pub const fn synthetic_ffa() -> Self {
        Self {
            status: EvidenceStatus::New,
            tick_ms: 10,
            world_width: 1000.0,
            world_height: 1000.0,
            move_units_per_ms: 0.2,
            projectile_units_per_ms: 2.0,
            projectile_lifetime_ms: 1500,
            hit_radius: 8.0,
            max_health: 100,
            damage_per_hit: 50,
            fire_cooldown_ms: 10,
            reload_ms: 100,
            max_ammo: 4,
            respawn_ms: 20,
            score_limit: 2,
        }
    }

    pub fn validate(self) -> Result<Self, Error> {
        if self.status == EvidenceStatus::Unresolved
            || self.tick_ms == 0
            || self.projectile_lifetime_ms == 0
            || self.fire_cooldown_ms == 0
            || self.reload_ms == 0
            || self.respawn_ms == 0
            || self.score_limit == 0
            || self.max_ammo == 0
            || self.max_health <= 0
            || self.damage_per_hit <= 0
            || ![
                self.world_width, self.world_height,
                self.move_units_per_ms, self.projectile_units_per_ms,
                self.hit_radius,
            ]
            .iter()
            .all(|x| x.is_finite() && *x > 0.0)
        {
            return Err(Error::InvalidRules);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlayerId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    /// Half of the raw horizontal key direction, from client event "4".
    pub hdt: f64,
    /// Half of the raw vertical key direction, from client event "4".
    pub vdt: f64,
    pub isn: u64,
    /// Observed client timestamp, not trusted for authoritative simulation dt.
    pub client_ts_ms: u64,
    pub jump: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub id: PlayerId,
    pub x: f64,
    pub y: f64,
    pub health: i32,
    pub score: u32,
    pub alive: bool,
    pub ammo: u16,
    /// The last accepted sequence, echoed in the future "rsd" adapter.
    pub last_processed_isn: Option<u64>,
    pub last_client_ts_ms: Option<u64>,
    pub spawn_x: f64,
    pub spawn_y: f64,
    last_input_tick: Option<u64>,
    last_fire_ms: Option<u64>,
    respawn_at_ms: Option<u64>,
    reload_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub owner: PlayerId,
    pub x: f64,
    pub y: f64,
    pub angle_rad: f64,
    pub remaining_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Running,
    Finished { winner: PlayerId },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Joined(PlayerId),
    Movement { id: PlayerId, isn: u64, x: f64, y: f64 },
    Fired { owner: PlayerId },
    Damaged { owner: PlayerId, target: PlayerId, health: i32 },
    Killed { killer: PlayerId, victim: PlayerId },
    Reloaded(PlayerId),
    Respawned(PlayerId),
    RoundEnded { winner: PlayerId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRules,
    DuplicatePlayer,
    MissingPlayer,
    InvalidInput,
    StaleInput,
    ExcessInput,
    DeadPlayer,
    Cooldown,
    NoAmmo,
    RoundFinished,
}

/// A deterministic simulation. World bounds, hits and spawn policy are NEW,
/// swappable engineering approximations. Never authenticate them as original.
pub struct Match {
    pub rules: Rules,
    pub now_ms: u64,
    pub phase: Phase,
    pub players: BTreeMap<PlayerId, Player>,
    pub projectiles: Vec<Projectile>,
    pub events: Vec<Event>,
}

impl Match {
    pub fn new(rules: Rules) -> Result<Self, Error> {
        let rules = rules.validate()?;
        Ok(Self {
            rules,
            now_ms: 0,
            phase: Phase::Running,
            players: BTreeMap::new(),
            projectiles: Vec::new(),
            events: Vec::new(),
        })
    }

    pub fn join(&mut self, id: PlayerId, x: f64, y: f64) -> Result<(), Error> {
        if self.phase != Phase::Running {
            return Err(Error::RoundFinished);
        }
        if !x.is_finite() || !y.is_finite()
            || !(0.0..=self.rules.world_width).contains(&x)
            || !(0.0..=self.rules.world_height).contains(&y)
        {
            return Err(Error::InvalidInput);
        }
        if self.players.contains_key(&id) {
            return Err(Error::DuplicatePlayer);
        }
        self.players.insert(id, Player {
            id, x, y, health: self.rules.max_health, score: 0,
            alive: true, ammo: self.rules.max_ammo,
            last_processed_isn: None, last_client_ts_ms: None,
            spawn_x: x, spawn_y: y,
            last_input_tick: None, last_fire_ms: None,
            respawn_at_ms: None, reload_at_ms: None,
        });
        self.events.push(Event::Joined(id));
        Ok(())
    }

    pub fn disconnect(&mut self, id: PlayerId) -> Result<(), Error> {
        if self.players.remove(&id).is_none() {
            return Err(Error::MissingPlayer);
        }
        self.projectiles.retain(|p| p.owner != id);
        Ok(())
    }

    /// Only one input accepted per server tick per player (NEW anti-abuse
    /// policy). One unique isn moves a fixed step; the client timestamp is
    /// recorded, but does not grant extra distance or time authority.
    pub fn input(&mut self, id: PlayerId, input: Input) -> Result<(), Error> {
        if self.phase != Phase::Running {
            return Err(Error::RoundFinished);
        }
        if !input.hdt.is_finite() || !input.vdt.is_finite()
            || input.hdt.abs() > 0.5 || input.vdt.abs() > 0.5
        {
            return Err(Error::InvalidInput);
        }
        let p = self.players.get_mut(&id).ok_or(Error::MissingPlayer)?;
        if !p.alive {
            return Err(Error::DeadPlayer);
        }
        if p.last_processed_isn.is_some_and(|n| input.isn <= n) {
            return Err(Error::StaleInput);
        }
        if p.last_input_tick == Some(self.now_ms) {
            return Err(Error::ExcessInput);
        }
        let len = input.hdt.hypot(input.vdt);
        let (dx, dy) = if len == 0.0 {
            (0.0, 0.0)
        } else {
            let distance = self.rules.move_units_per_ms * self.rules.tick_ms as f64;
            (input.hdt / len * distance, input.vdt / len * distance)
        };
        p.x = (p.x + dx).clamp(0.0, self.rules.world_width);
        p.y = (p.y + dy).clamp(0.0, self.rules.world_height);
        p.last_processed_isn = Some(input.isn);
        p.last_client_ts_ms = Some(input.client_ts_ms);
        p.last_input_tick = Some(self.now_ms);
        self.events.push(Event::Movement { id, isn: input.isn, x: p.x, y: p.y });
        Ok(())
    }

    /// A new directional projectile policy. Does not trust claimed shot
    /// position, client timestamp or hit notification for damage authority.
    pub fn fire(&mut self, id: PlayerId, angle_rad: f64) -> Result<(), Error> {
        if self.phase != Phase::Running {
            return Err(Error::RoundFinished);
        }
        if !angle_rad.is_finite() || !(-2.0 * PI..=2.0 * PI).contains(&angle_rad) {
            return Err(Error::InvalidInput);
        }
        let p = self.players.get_mut(&id).ok_or(Error::MissingPlayer)?;
        if !p.alive {
            return Err(Error::DeadPlayer);
        }
        if p.last_fire_ms.is_some_and(|last|
            self.now_ms.saturating_sub(last) < self.rules.fire_cooldown_ms)
            || p.reload_at_ms.is_some()
        {
            return Err(Error::Cooldown);
        }
        if p.ammo == 0 {
            return Err(Error::NoAmmo);
        }
        p.ammo -= 1;
        p.last_fire_ms = Some(self.now_ms);
        if p.ammo == 0 {
            p.reload_at_ms = Some(self.now_ms.saturating_add(self.rules.reload_ms));
        }
        self.projectiles.push(Projectile {
            owner: id, x: p.x, y: p.y, angle_rad,
            remaining_ms: self.rules.projectile_lifetime_ms,
        });
        self.events.push(Event::Fired { owner: id });
        Ok(())
    }

    /// Advance exactly one server-owned fixed tick. BTreeMap ordering and
    /// fixed player/projectile iteration order make replays reproducible.
    pub fn tick(&mut self) {
        self.now_ms = self.now_ms.saturating_add(self.rules.tick_ms);
        if self.phase != Phase::Running {
            return;
        }
        for (id, player) in &mut self.players {
            if let Some(due) = player.reload_at_ms {
                if self.now_ms >= due {
                    player.ammo = self.rules.max_ammo;
                    player.reload_at_ms = None;
                    self.events.push(Event::Reloaded(*id));
                }
            }
            if let Some(due) = player.respawn_at_ms {
                if self.now_ms >= due {
                    player.alive = true;
                    player.health = self.rules.max_health;
                    player.x = player.spawn_x;
                    player.y = player.spawn_y;
                    player.ammo = self.rules.max_ammo;
                    player.respawn_at_ms = None;
                    player.reload_at_ms = None;
                    player.last_fire_ms = None;
                    player.last_input_tick = None;
                    self.events.push(Event::Respawned(*id));
                }
            }
        }

        let mut hits = Vec::new();
        let dt = self.rules.tick_ms;
        for projectile in &mut self.projectiles {
            if projectile.remaining_ms <= dt {
                projectile.remaining_ms = 0;
                continue;
            }
            projectile.remaining_ms -= dt;
            projectile.x += self.rules.projectile_units_per_ms
                * dt as f64 * projectile.angle_rad.cos();
            projectile.y += self.rules.projectile_units_per_ms
                * dt as f64 * projectile.angle_rad.sin();
            if projectile.x < 0.0 || projectile.y < 0.0
                || projectile.x > self.rules.world_width
                || projectile.y > self.rules.world_height
            {
                projectile.remaining_ms = 0;
                continue;
            }
            for (id, target) in &self.players {
                if *id == projectile.owner || !target.alive {
                    continue;
                }
                let radius = (projectile.x - target.x).hypot(projectile.y - target.y);
                if radius <= self.rules.hit_radius {
                    hits.push((projectile.owner, *id));
                    projectile.remaining_ms = 0;
                    break;
                }
            }
        }
        self.projectiles.retain(|p| p.remaining_ms != 0);
        for (owner, target) in hits {
            if self.phase != Phase::Running {
                break;
            }
            // A player may have been hit by multiple projectiles in one tick.
            if !self.players.get(&target).is_some_and(|p| p.alive) {
                continue;
            }
            let p = self.players.get_mut(&target).expect("previously checked target");
            p.health = (p.health - self.rules.damage_per_hit).max(0);
            self.events.push(Event::Damaged { owner, target, health: p.health });
            if p.health == 0 {
                p.alive = false;
                p.respawn_at_ms = Some(self.now_ms.saturating_add(self.rules.respawn_ms));
                self.events.push(Event::Killed { killer: owner, victim: target });
                if let Some(killer) = self.players.get_mut(&owner) {
                    killer.score += 1;
                    if killer.score >= self.rules.score_limit {
                        self.phase = Phase::Finished { winner: owner };
                        self.events.push(Event::RoundEnded { winner: owner });
                    }
                }
            }
        }
    }

    /// Deterministic event list since match creation. Consumers can persist
    /// an exact replay; wire adaptation must not treat these as original events.
    #[must_use]
    pub fn event_log(&self) -> &[Event] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(limit: u32) -> Match {
        let mut rules = Rules::synthetic_ffa();
        rules.score_limit = limit;
        let mut world = Match::new(rules).unwrap();
        world.join(PlayerId(3), 10.0, 10.0).unwrap();
        world.join(PlayerId(4), 30.0, 10.0).unwrap();
        world
    }

    fn shot_that_hits(world: &mut Match) {
        world.fire(PlayerId(3), 0.0).unwrap();
        world.tick();
    }

    #[test]
    fn all_server_owned_numbers_are_labeled_new() {
        let rules = Rules::synthetic_ffa();
        assert_eq!(rules.status, EvidenceStatus::New);
        assert_eq!(rules.validate(), Ok(rules));
        let mut bad = rules;
        bad.status = EvidenceStatus::Unresolved;
        assert_eq!(bad.validate(), Err(Error::InvalidRules));
        bad = rules;
        bad.projectile_units_per_ms = f64::NAN;
        assert_eq!(bad.validate(), Err(Error::InvalidRules));
    }

    #[test]
    fn movement_normalizes_and_rejects_replay_and_clock_abuse() {
        let mut w = setup(2);
        let inp = Input { hdt: 0.5, vdt: 0.5, isn: 7, client_ts_ms: u64::MAX, jump: false };
        w.input(PlayerId(3), inp).unwrap();
        let p = &w.players[&PlayerId(3)];
        assert!(((p.x - 10.0).hypot(p.y - 10.0) - 2.0).abs() < 1e-9);
        assert_eq!(p.last_processed_isn, Some(7));
        assert_eq!(w.input(PlayerId(3), inp), Err(Error::StaleInput));
        let next = Input { isn: 8, ..inp };
        assert_eq!(w.input(PlayerId(3), next), Err(Error::ExcessInput));
        w.tick();
        w.input(PlayerId(3), next).unwrap();
    }

    #[test]
    fn projectile_hit_damage_kill_respawn_and_repeatable_round_end() {
        let mut w = setup(2);
        shot_that_hits(&mut w);
        assert_eq!(w.players[&PlayerId(4)].health, 50);
        shot_that_hits(&mut w);
        assert!(!w.players[&PlayerId(4)].alive);
        assert_eq!(w.players[&PlayerId(3)].score, 1);
        assert_eq!(w.fire(PlayerId(4), PI), Err(Error::DeadPlayer));
        w.tick();
        w.tick();
        assert!(w.players[&PlayerId(4)].alive);
        assert_eq!(w.players[&PlayerId(4)].health, 100);
        w.tick(); // until shooter cooldown is over
        shot_that_hits(&mut w);
        shot_that_hits(&mut w);
        assert_eq!(w.phase, Phase::Finished { winner: PlayerId(3) });
        assert_eq!(w.fire(PlayerId(3), 0.0), Err(Error::RoundFinished));
        assert!(w.event_log().iter().any(|e| matches!(e, Event::Respawned(PlayerId(4)))));
        assert_eq!(w.event_log().iter().filter(|e| matches!(e, Event::Killed { .. })).count(), 2);
    }

    #[test]
    fn same_script_produces_exact_event_sequence() {
        fn run() -> Vec<Event> {
            let mut m = setup(1);
            shot_that_hits(&mut m);
            shot_that_hits(&mut m);
            m.events
        }
        assert_eq!(run(), run());
    }

    #[test]
    fn illegal_shots_and_disconnect_are_safe() {
        let mut w = setup(2);
        assert_eq!(w.fire(PlayerId(3), f64::NAN), Err(Error::InvalidInput));
        w.fire(PlayerId(3), 0.0).unwrap();
        assert_eq!(w.fire(PlayerId(3), 0.0), Err(Error::Cooldown));
        w.disconnect(PlayerId(3)).unwrap();
        assert!(!w.players.contains_key(&PlayerId(3)));
        assert!(w.projectiles.is_empty());
        assert_eq!(w.disconnect(PlayerId(3)), Err(Error::MissingPlayer));
    }

    #[test]
    fn boundary_inputs_and_unique_player_ids() {
        let mut w = setup(2);
        assert_eq!(w.join(PlayerId(3), 50.0, 50.0), Err(Error::DuplicatePlayer));
        assert_eq!(w.join(PlayerId(7), -1.0, 5.0), Err(Error::InvalidInput));
        assert_eq!(w.input(PlayerId(3), Input {
            hdt: 2.0, vdt: 0.0, isn: 1, client_ts_ms: 0, jump: false,
        }), Err(Error::InvalidInput));
    }
}

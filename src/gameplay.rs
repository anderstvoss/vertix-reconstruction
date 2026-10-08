//! Provisional authoritative FFA simulation for the 2016-08-06 wire contract.
//!
//! Original-client-derived: class order and weapon indices, per-slot reload
//! state, movement-input encoding. Server-side combat and balancing are NEW /
//! PROVISIONAL, NOT the unavailable original authoritative game logic.
//! No client-supplied hit, ammo, reload-complete or damage value is trusted.
//! Transport, world collision and map visibility remain separate interfaces.
use std::collections::BTreeMap;

const HIT_RADIUS: f64 = 16.0;
const MAX_INPUT_DELTA_MS: u64 = 100;
const PLAYER_SPEED_PX_PER_MS: f64 = 0.18; // PROVISIONAL, not recovered.
const MAX_SHOT_DISTANCE: f64 = 1_200.0; // PROVISIONAL.

/// Archived client 2016-08-06 roster. Numeric combat attributes are provisional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassLoadout {
    pub name: &'static str,
    pub weapon_indices: &'static [u8],
}

pub const CLASSES: [ClassLoadout; 9] = [
    ClassLoadout { name: "Triggerman", weapon_indices: &[0, 5] },
    ClassLoadout { name: "Detective", weapon_indices: &[1, 5] },
    ClassLoadout { name: "Hunter", weapon_indices: &[2, 7] },
    ClassLoadout { name: "Run 'N Gun", weapon_indices: &[3] },
    ClassLoadout { name: "Vince", weapon_indices: &[4, 5] },
    ClassLoadout { name: "Rocketeer", weapon_indices: &[6] },
    ClassLoadout { name: "Spray N' Pray", weapon_indices: &[8] },
    ClassLoadout { name: "Arsonist", weapon_indices: &[9] },
    ClassLoadout { name: "Duck", weapon_indices: &[9] },
];

/// Inferred/reconstruction-team base. Never claim these are original 2016 server stats.
/// Per-era overrides should replace this table once justified by stronger evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponSpec {
    pub weapon_index: u8,
    pub magazine: u16,
    pub reload_ms: u64,
    pub cooldown_ms: u64,
    pub damage: u16,
}

pub const PROVISIONAL_WEAPONS: [WeaponSpec; 10] = [
    WeaponSpec { weapon_index: 0, magazine: 24, reload_ms: 1500, cooldown_ms: 143, damage: 20 },
    WeaponSpec { weapon_index: 1, magazine: 6, reload_ms: 2200, cooldown_ms: 390, damage: 50 },
    WeaponSpec { weapon_index: 2, magazine: 1, reload_ms: 1200, cooldown_ms: 1200, damage: 100 },
    WeaponSpec { weapon_index: 3, magazine: 8, reload_ms: 1000, cooldown_ms: 78, damage: 17 },
    WeaponSpec { weapon_index: 4, magazine: 3, reload_ms: 2000, cooldown_ms: 143, damage: 22 },
    WeaponSpec { weapon_index: 5, magazine: 1, reload_ms: 2000, cooldown_ms: 1200, damage: 110 },
    WeaponSpec { weapon_index: 6, magazine: 1, reload_ms: 1200, cooldown_ms: 1200, damage: 110 },
    WeaponSpec { weapon_index: 7, magazine: 6, reload_ms: 900, cooldown_ms: 120, damage: 12 },
    WeaponSpec { weapon_index: 8, magazine: 50, reload_ms: 2000, cooldown_ms: 90, damage: 8 },
    WeaponSpec { weapon_index: 9, magazine: 30, reload_ms: 1200, cooldown_ms: 70, damage: 10 },
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Position {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponState {
    pub spec: WeaponSpec,
    pub ammo: u16,
    pub reload_complete_ms: Option<u64>,
    pub last_fired_ms: Option<u64>,
}

impl WeaponState {
    #[must_use]
    pub const fn new(spec: WeaponSpec) -> Self {
        Self {
            ammo: spec.magazine,
            spec,
            reload_complete_ms: None,
            last_fired_ms: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    pub id: u64,
    pub class_index: usize,
    pub position: Position,
    pub health: u16,
    pub max_health: u16,
    pub alive: bool,
    pub score: u32,
    pub kills: u32,
    pub deaths: u32,
    pub current_weapon: usize,
    pub weapons: Vec<WeaponState>,
    pub last_input_sequence: Option<u64>,
    last_input_ts_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    UnknownPlayer,
    InvalidClass,
    InvalidSlot,
    Dead,
    NoAmmo,
    Reloading,
    Cooldown,
    InvalidInput,
    StaleInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadStatus {
    Started { ready_at_ms: u64 },
    AlreadyReloading { ready_at_ms: u64 },
    MagazineFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FireResult {
    pub shooter: u64,
    pub weapon_slot: usize,
    pub ammo: u16,
    pub hit_player: Option<u64>,
    pub damage: u16,
    pub killed: bool,
}

#[derive(Clone, Debug)]
pub struct Match {
    pub players: BTreeMap<u64, Player>,
    next_player_id: u64,
    pub weapon_specs: [WeaponSpec; 10],
    pub default_max_health: u16,
}

impl Default for Match {
    fn default() -> Self {
        Self {
            players: BTreeMap::new(),
            next_player_id: 1,
            weapon_specs: PROVISIONAL_WEAPONS,
            default_max_health: 100,
        }
    }
}

impl Match {
    /// Spawn a new participant with an independent magazine/timer per loadout slot.
    /// Reject special boss indices until explicitly modeled.
    pub fn join(&mut self, class_index: usize, position: Position) -> Result<u64, Reject> {
        let class = CLASSES.get(class_index).ok_or(Reject::InvalidClass)?;
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(Reject::InvalidInput);
        }
        let weapons = class.weapon_indices.iter().map(|&weapon_index| {
            let mut spec = self.weapon_specs[usize::from(weapon_index)];
            // Duck's "Jump" is not a damaging flamethrower even though its
            // client sprite/weapon index overlaps 9 in the recovered roster.
            if class_index == 8 {
                spec.magazine = 0;
                spec.damage = 0;
            }
            WeaponState::new(spec)
        }).collect();
        let id = self.next_player_id;
        self.next_player_id += 1;
        self.players.insert(id, Player {
            id,
            class_index,
            position,
            health: self.default_max_health,
            max_health: self.default_max_health,
            alive: true,
            score: 0,
            kills: 0,
            deaths: 0,
            current_weapon: 0,
            weapons,
            last_input_sequence: None,
            last_input_ts_ms: None,
        });
        Ok(id)
    }

    pub fn leave(&mut self, id: u64) -> Result<(), Reject> {
        self.players.remove(&id).map(|_| ()).ok_or(Reject::UnknownPlayer)
    }

    pub fn switch_weapon(&mut self, id: u64, slot: usize) -> Result<(), Reject> {
        let player = self.players.get_mut(&id).ok_or(Reject::UnknownPlayer)?;
        if slot >= player.weapons.len() {
            return Err(Reject::InvalidSlot);
        }
        player.current_weapon = slot;
        Ok(())
    }

    /// Idempotent start; reload is on the selected weapon, not a global timer.
    pub fn reload(&mut self, id: u64, now_ms: u64) -> Result<ReloadStatus, Reject> {
        let player = self.players.get_mut(&id).ok_or(Reject::UnknownPlayer)?;
        if !player.alive {
            return Err(Reject::Dead);
        }
        let weapon = &mut player.weapons[player.current_weapon];
        if let Some(ready_at_ms) = weapon.reload_complete_ms {
            return Ok(ReloadStatus::AlreadyReloading { ready_at_ms });
        }
        if weapon.ammo == weapon.spec.magazine {
            return Ok(ReloadStatus::MagazineFull);
        }
        let ready_at_ms = now_ms.saturating_add(weapon.spec.reload_ms);
        weapon.reload_complete_ms = Some(ready_at_ms);
        Ok(ReloadStatus::Started { ready_at_ms })
    }

    /// Advance all reloads independently, including unselected weapon slots.
    pub fn tick(&mut self, now_ms: u64) {
        for player in self.players.values_mut() {
            for weapon in &mut player.weapons {
                if weapon.reload_complete_ms.is_some_and(|at| now_ms >= at) {
                    weapon.ammo = weapon.spec.magazine;
                    weapon.reload_complete_ms = None;
                }
            }
        }
    }

    /// Client intent is a direction, never an asserted victim or damage value.
    /// The server owns player coordinates and selects nearest circular hitbox.
    /// LOS, projectile travel/bounce/pierce and lag compensation are NOT modeled.
    pub fn fire(
        &mut self,
        shooter: u64,
        angle_radians: f64,
        distance: f64,
        now_ms: u64,
    ) -> Result<FireResult, Reject> {
        if !angle_radians.is_finite() || !distance.is_finite() || distance < 0.0 {
            return Err(Reject::InvalidInput);
        }
        let (origin, damage, slot, ammo) = {
            let player = self.players.get_mut(&shooter).ok_or(Reject::UnknownPlayer)?;
            if !player.alive {
                return Err(Reject::Dead);
            }
            let slot = player.current_weapon;
            let weapon = &mut player.weapons[slot];
            if weapon.reload_complete_ms.is_some() {
                return Err(Reject::Reloading);
            }
            if weapon.ammo == 0 {
                return Err(Reject::NoAmmo);
            }
            if weapon.last_fired_ms.is_some_and(|last| {
                now_ms < last || now_ms - last < weapon.spec.cooldown_ms
            }) {
                return Err(Reject::Cooldown);
            }
            weapon.ammo -= 1;
            weapon.last_fired_ms = Some(now_ms);
            (player.position, weapon.spec.damage, slot, weapon.ammo)
        };
        let range = distance.min(MAX_SHOT_DISTANCE);
        let direction = (angle_radians.cos(), angle_radians.sin());
        let mut nearest: Option<(u64, f64)> = None;
        for (&id, target) in &self.players {
            if id == shooter || !target.alive {
                continue;
            }
            let delta_x = target.position.x - origin.x;
            let delta_y = target.position.y - origin.y;
            let along = delta_x.mul_add(direction.0, delta_y * direction.1);
            if !(0.0..=range).contains(&along) {
                continue;
            }
            let sideways = delta_x.mul_add(direction.1, -(delta_y * direction.0));
            if sideways.abs() <= HIT_RADIUS && nearest.is_none_or(|(_, d)| along < d) {
                nearest = Some((id, along));
            }
        }
        let mut result = FireResult {
            shooter,
            weapon_slot: slot,
            ammo,
            hit_player: None,
            damage: 0,
            killed: false,
        };
        if let Some((id, _)) = nearest {
            let target = self.players.get_mut(&id).expect("target was selected from player map");
            let applied = damage.min(target.health);
            target.health -= applied;
            if target.health == 0 {
                target.alive = false;
                target.deaths += 1;
                result.killed = true;
            }
            result.hit_player = Some(id);
            result.damage = applied;
            if result.killed {
                let attacker = self.players.get_mut(&shooter).expect("validated attacker");
                attacker.kills += 1;
                attacker.score += 1;
            }
        }
        Ok(result)
    }

    /// Source-compatible direction scaling: hdt/vdt in {-0.5, 0, 0.5}.
    /// The original client normalizes diagonals and transmits timestamps / isn.
    /// Caps elapsed time to reduce a trivial client-clock speed exploit.
    pub fn input(
        &mut self,
        id: u64,
        hdt: f64,
        vdt: f64,
        ts_ms: u64,
        isn: u64,
    ) -> Result<Position, Reject> {
        if !matches!(hdt, -0.5 | 0.0 | 0.5) || !matches!(vdt, -0.5 | 0.0 | 0.5) {
            return Err(Reject::InvalidInput);
        }
        let player = self.players.get_mut(&id).ok_or(Reject::UnknownPlayer)?;
        if !player.alive {
            return Err(Reject::Dead);
        }
        if player.last_input_sequence.is_some_and(|last| isn <= last)
            || player.last_input_ts_ms.is_some_and(|last| ts_ms < last)
        {
            return Err(Reject::StaleInput);
        }
        let elapsed = player.last_input_ts_ms.map_or(0, |last| ts_ms.saturating_sub(last));
        let delta = elapsed.min(MAX_INPUT_DELTA_MS) as f64;
        let length = hdt.hypot(vdt);
        if length > 0.0 {
            player.position.x += hdt / length * PLAYER_SPEED_PX_PER_MS * delta;
            player.position.y += vdt / length * PLAYER_SPEED_PX_PER_MS * delta;
        }
        player.last_input_ts_ms = Some(ts_ms);
        player.last_input_sequence = Some(isn);
        Ok(player.position)
    }

    /// Guest respawn. Retains score; refills and resets all weapon timers.
    pub fn respawn(&mut self, id: u64, position: Position) -> Result<(), Reject> {
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(Reject::InvalidInput);
        }
        let player = self.players.get_mut(&id).ok_or(Reject::UnknownPlayer)?;
        player.position = position;
        player.health = player.max_health;
        player.alive = true;
        player.current_weapon = 0;
        player.last_input_ts_ms = None;
        player.last_input_sequence = None;
        for weapon in &mut player.weapons {
            weapon.ammo = weapon.spec.magazine;
            weapon.reload_complete_ms = None;
            weapon.last_fired_ms = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_and_loadout_slots_are_2016_08_06_specific() {
        assert_eq!(CLASSES.len(), 9);
        assert_eq!(CLASSES[0].weapon_indices, &[0, 5]);
        assert_eq!(CLASSES[2].weapon_indices, &[2, 7]);
        assert_eq!(CLASSES[4].weapon_indices, &[4, 5]);
        let mut world = Match::default();
        let hunter = world.join(2, Position::new(0.0, 0.0)).unwrap();
        assert_eq!(world.players[&hunter].weapons.len(), 2);
        assert_eq!(world.players[&hunter].weapons[1].spec.weapon_index, 7);
        assert_eq!(world.join(9, Position::new(0.0, 0.0)), Err(Reject::InvalidClass));
    }

    #[test]
    fn reload_uses_its_own_slot_and_acknowledges_only_at_deadline() {
        let mut world = Match::default();
        let shooter = world.join(0, Position::new(0.0, 0.0)).unwrap();
        world.fire(shooter, 0.0, 0.0, 1000).unwrap();
        assert_eq!(world.players[&shooter].weapons[0].ammo, 23);
        assert_eq!(world.reload(shooter, 1100).unwrap(), ReloadStatus::Started { ready_at_ms: 2600 });
        assert_eq!(world.reload(shooter, 1200).unwrap(), ReloadStatus::AlreadyReloading { ready_at_ms: 2600 });
        assert_eq!(world.fire(shooter, 0.0, 10.0, 1400), Err(Reject::Reloading));
        world.switch_weapon(shooter, 1).unwrap();
        assert_eq!(world.players[&shooter].weapons[1].ammo, 1);
        world.tick(2599);
        assert_eq!(world.players[&shooter].weapons[0].ammo, 23);
        world.tick(2600);
        assert_eq!(world.players[&shooter].weapons[0].ammo, 24);
        assert!(world.players[&shooter].weapons[0].reload_complete_ms.is_none());
        world.switch_weapon(shooter, 0).unwrap();
        assert_eq!(world.reload(shooter, 2600), Ok(ReloadStatus::MagazineFull));
    }

    #[test]
    fn damage_is_authoritative_and_kills_are_idempotent() {
        let mut world = Match::default();
        let a = world.join(0, Position::new(0.0, 0.0)).unwrap();
        let b = world.join(1, Position::new(90.0, 0.0)).unwrap();
        for shot in 0..5 {
            let time = 1000 + shot * 143;
            let result = world.fire(a, 0.0, 200.0, time).unwrap();
            assert_eq!(result.hit_player, Some(b));
            assert_eq!(result.damage, 20);
            assert_eq!(result.killed, shot == 4);
        }
        assert_eq!(world.players[&b].health, 0);
        assert_eq!(world.players[&a].kills, 1);
        assert_eq!(world.players[&a].score, 1);
        assert_eq!(world.players[&b].deaths, 1);
        let repeat = world.fire(a, 0.0, 200.0, 1000 + 5 * 143).unwrap();
        assert_eq!(repeat.hit_player, None);
        assert_eq!(world.players[&a].score, 1);
    }

    #[test]
    fn fire_enforces_ammo_cooldown_and_closest_hit() {
        let mut world = Match::default();
        let a = world.join(0, Position::new(0.0, 0.0)).unwrap();
        let b = world.join(0, Position::new(30.0, 0.0)).unwrap();
        let c = world.join(0, Position::new(60.0, 0.0)).unwrap();
        assert_eq!(world.fire(a, 0.0, 100.0, 100), Ok(FireResult {
            shooter: a, weapon_slot: 0, ammo: 23, hit_player: Some(b),
            damage: 20, killed: false
        }));
        assert_eq!(world.fire(a, 0.0, 100.0, 101), Err(Reject::Cooldown));
        assert_eq!(world.players[&c].health, 100);
        assert_eq!(world.fire(a, f64::NAN, 100.0, 1000), Err(Reject::InvalidInput));
        assert_eq!(world.fire(a, std::f64::consts::PI, 100.0, 1000).unwrap().hit_player, None);
    }

    #[test]
    fn ordered_movement_cancels_opposite_intents_and_normalizes_diagonals() {
        let mut world = Match::default();
        let id = world.join(0, Position::new(0.0, 0.0)).unwrap();
        world.input(id, 0.0, -0.5, 1000, 1).unwrap();
        assert_eq!(world.input(id, 0.0, 0.0, 1010, 2).unwrap(), Position::new(0.0, 0.0));
        let moved = world.input(id, 0.0, -0.5, 1020, 3).unwrap();
        assert!((moved.y + 1.8).abs() < 1e-10);
        assert_eq!(world.input(id, 0.0, -0.5, 1020, 3), Err(Reject::StaleInput));
        assert_eq!(world.input(id, 1.0, 0.0, 1100, 4), Err(Reject::InvalidInput));
        let moved = world.input(id, 0.5, -0.5, 1120, 4).unwrap();
        assert!((moved.x - 12.727_922_061_357_855).abs() < 1e-7);
    }

    #[test]
    fn death_respawn_resets_only_combat_not_match_score() {
        let mut world = Match::default();
        let a = world.join(0, Position::new(0.0, 0.0)).unwrap();
        let b = world.join(0, Position::new(50.0, 0.0)).unwrap();
        for shot in 0..5 {
            world.fire(a, 0.0, 100.0, 1000 + shot * 143).unwrap();
        }
        world.respawn(b, Position::new(300.0, 300.0)).unwrap();
        assert_eq!(world.players[&b].health, 100);
        assert_eq!(world.players[&b].deaths, 1);
        assert!(world.players[&b].alive);
        assert_eq!(world.players[&a].score, 1);
    }
}

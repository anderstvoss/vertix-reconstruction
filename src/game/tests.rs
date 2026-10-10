//! The ported game rules, driven the way KRP's client drives them.

use std::f64::consts::PI;

use serde_json::{Value, json};

use super::*;
use crate::eio::codec::{Packet, decode_payload};
use crate::eio::{Query, Server, Timing};
use crate::sio::Event;
use data::committed;
use map::Map;
use room::Out;

const ARENA: &str = include_str!("../../data/maps/arena.txt");

fn arena() -> MapSet {
    MapSet::new(vec![maps::MapEntry {
        id: "arena".into(),
        source: "arena.txt".into(),
        map: Map::parse(ARENA).unwrap(),
    }])
    .unwrap()
}

/// One room and a clock.
struct Bench {
    room: Room,
    data: GameData,
    maps: MapSet,
    rules: Assumptions,
    now: f64,
}

impl Bench {
    fn new(mode: &str) -> Self {
        Self::with_balance(mode, "krp")
    }

    fn with_balance(mode: &str, balance: &str) -> Self {
        let data = committed(balance);
        let rules = assumptions::committed().0;
        let maps = arena();
        let mi = data.mode_index(mode).unwrap();
        let mut room = Room::new("T", &data, &maps, &rules.rules, &rules.world, mi, 7);
        // Barrels would make shots depend on the seed.
        room.world.clutter.clear();
        Self {
            room,
            data,
            maps,
            rules,
            now: 1000.0,
        }
    }

    fn join(&mut self) -> u32 {
        self.room.join(&self.data)
    }

    fn send(&mut self, i: u32, name: &str, args: Vec<Value>) -> Vec<Out> {
        let ev = Event::new(name, args);
        self.room
            .on_event(&self.data, &self.maps, &self.rules.rules, i, &ev, self.now);
        self.take()
    }

    /// The client's answer to `welcome`: spawn as `class`.
    fn spawn(&mut self, i: u32, name: &str, class: usize) -> Vec<Out> {
        self.send(
            i,
            "gotit",
            vec![
                json!({"name": name, "classIndex": class}),
                json!(false),
                json!(0),
                json!(false),
            ],
        )
    }

    fn take(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.room.out)
    }

    /// Advances `ms` in server ticks.
    fn run(&mut self, ms: f64) -> Vec<Out> {
        let dt = 1000.0 / self.rules.net.update_hz;
        let end = self.now + ms;
        let mut out = Vec::new();
        while self.now < end {
            self.now += dt;
            self.room
                .tick(&self.data, &self.maps, &self.rules.rules, self.now, dt);
            out.extend(self.take());
        }
        out
    }

    fn p(&self, i: u32) -> &room::Player {
        self.room.players.iter().find(|p| p.index == i).unwrap()
    }

    fn p_mut(&mut self, i: u32) -> &mut room::Player {
        self.room.players.iter_mut().find(|p| p.index == i).unwrap()
    }

    /// The middle of the floor tile at map cell (col, row).
    fn cell(&self, col: usize, row: usize) -> (f64, f64) {
        let s = self.room.world.scale;
        #[allow(clippy::cast_precision_loss)]
        let (c, r) = ((col as f64 - 2.0) * s, (row as f64 - 2.0) * s);
        (c + s / 2.0, r + s / 2.0)
    }
}

fn named<'a>(out: &'a [Out], name: &str) -> Vec<&'a Out> {
    out.iter().filter(|o| o.event.name == name).collect()
}

fn one<'a>(out: &'a [Out], name: &str) -> &'a Out {
    let all = named(out, name);
    assert_eq!(all.len(), 1, "one {name} in {:?}", names(out));
    all[0]
}

fn names(out: &[Out]) -> Vec<&str> {
    out.iter().map(|o| o.event.name.as_str()).collect()
}

#[test]
fn joining_sends_the_room_welcome_and_catalogues() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    let out = b.take();
    assert_eq!(
        names(&out),
        ["yourRoom", "welcome", "updHt", "updShrt", "updCmo"]
    );
    assert!(out.iter().all(|o| o.to == To::One(i)));
    let w = &one(&out, "welcome").event.args;
    assert_eq!(w[0]["id"], json!(i));
    assert_eq!(w[1], json!(true), "first welcome opens the menu");
    let hats = one(&out, "updHt").event.args[1].as_array().unwrap();
    assert!(!hats.is_empty());
    assert!(hats.iter().all(|h| h["count"] == json!(0)));
    // Ordered rarest last, as KRP sorts them.
    let ch: Vec<f64> = hats
        .iter()
        .map(|h| h["chance"].as_f64().unwrap_or(0.0))
        .collect();
    assert!(ch.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn spawning_sets_up_the_game_and_protects_briefly() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    b.take();
    let out = b.spawn(i, "<b>Tester</b>", 0);
    let setup = one(&out, "gameSetup");
    assert_eq!(setup.to, To::One(i));
    let doc: Value = serde_json::from_str(setup.event.args[0].as_str().unwrap()).unwrap();
    assert_eq!(doc["you"]["name"], "Tester", "markup is stripped");
    assert_eq!(
        doc["mapData"]["tiles"],
        json!([]),
        "the client builds tiles"
    );
    assert_eq!(doc["tileScale"], json!(b.room.world.scale));
    assert_eq!(one(&out, "6").event.args[0], json!("Free For All"));
    assert_eq!(one(&out, "add").to, To::All);
    assert!(!named(&out, "ts").is_empty() && !named(&out, "lb").is_empty());

    let p = b.p(i);
    assert!(!p.dead && p.spawn_protected);
    let red = b
        .room
        .world
        .spawn_tiles
        .iter()
        .map(|&t| &b.room.world.tiles[t])
        .any(|t| {
            (t.x + t.scale / 2.0 - p.x).abs() < 1e-9 && (t.y + t.scale / 2.0 - p.y).abs() < 1e-9
        });
    assert!(red, "spawned on a spawn tile");

    // The mode intro shows once, not on every respawn.
    let again = b.spawn(i, "Tester", 0);
    assert!(named(&again, "6").is_empty());

    let ms = b.rules.rules.spawn_protection_ms;
    let later = b.run(ms + 100.0);
    let sp = named(&later, "upd");
    assert!(
        sp.iter()
            .any(|o| o.event.args[0] == json!({"i": i, "sp": false}))
    );
    assert!(!b.p(i).spawn_protected);
}

#[test]
fn modes_force_their_class() {
    for (mode, class) in [("snipe", 2), ("rckt", 5), ("pyro", 7)] {
        let mut b = Bench::new(mode);
        let i = b.join();
        b.spawn(i, "x", 0);
        assert_eq!(b.p(i).class_index, class, "{mode}");
    }
    // The boss mode makes the blue player the boss.
    let mut b = Bench::new("boss");
    let i = b.join();
    assert_eq!(b.p(i).team, "blue");
    b.spawn(i, "x", 0);
    assert!(b.p(i).is_boss);
    assert_eq!(b.p(i).class_index, 10);
    let j = b.join();
    assert_eq!(b.p(j).team, "red");
}

#[test]
fn input_moves_by_class_speed_and_answers_with_positions() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    b.spawn(i, "x", 0);
    let (x, y) = b.cell(8, 7);
    b.p_mut(i).x = x;
    b.p_mut(i).y = y;
    let speed = b.p(i).speed;
    let out = b.send(
        i,
        "4",
        vec![json!({"hdt": 1, "vdt": 0, "delta": 16, "isn": 9, "s": 0})],
    );
    assert!((b.p(i).x - (x + speed * 16.0).round()).abs() < 1e-9);
    let rsd = one(&out, "rsd");
    assert_eq!(rsd.to, To::One(i));
    assert_eq!(rsd.event.args[0][5], json!(9), "echoes the input number");

    // Diagonals are normalised, and a huge frame delta is capped.
    let before = (b.p(i).x, b.p(i).y);
    b.send(
        i,
        "4",
        vec![json!({"hdt": 1, "vdt": 1, "delta": 1e9, "isn": 10})],
    );
    let moved = (b.p(i).x - before.0).hypot(b.p(i).y - before.1);
    assert!(moved <= speed * 100.0 + 2.0, "moved {moved}");
}

#[test]
fn walls_stop_players() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    b.spawn(i, "x", 0);
    for _ in 0..200 {
        b.send(
            i,
            "4",
            vec![json!({"hdt": -1, "vdt": -1, "delta": 16, "isn": 0})],
        );
    }
    let p = b.p(i);
    assert!(
        p.x - p.width / 2.0 >= -1.0 && p.y >= 0.0,
        "inside: {} {}",
        p.x,
        p.y
    );
}

#[test]
fn jumping_tells_everyone_and_lands() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    b.spawn(i, "x", 0);
    let out = b.send(
        i,
        "4",
        vec![json!({"hdt": 0, "vdt": 0, "delta": 16, "isn": 0, "s": 1})],
    );
    assert_eq!(one(&out, "jum").to, To::All);
    assert!(b.p(i).jump_y > 0.0);
    for _ in 0..200 {
        b.send(
            i,
            "4",
            vec![json!({"hdt": 0, "vdt": 0, "delta": 16, "isn": 0, "s": 0})],
        );
    }
    assert!(b.p(i).jump_y.abs() < f64::EPSILON);
}

/// Two players facing each other along a row, shooter on the left.
fn duel(b: &mut Bench, class: usize) -> (u32, u32) {
    let a = b.join();
    let v = b.join();
    b.spawn(a, "Ann", class);
    b.spawn(v, "Vic", 0);
    let (ax, ay) = b.cell(6, 7);
    let (vx, _) = b.cell(12, 7);
    for (i, x) in [(a, ax), (v, vx)] {
        let p = b.p_mut(i);
        p.x = x;
        p.y = ay;
        p.spawn_protected = false;
    }
    b.take();
    (a, v)
}

fn fire_right(b: &mut Bench, a: u32) -> Vec<Out> {
    let p = b.p(a).clone();
    // The client aims with targetF; the shot leaves at targetF + PI.
    b.send(a, "0", vec![json!(-PI)]);
    b.send(
        a,
        "1",
        vec![
            json!(p.x),
            json!(p.y),
            json!(p.jump_y),
            json!(-PI),
            json!(600),
        ],
    )
}

#[test]
fn shots_hit_and_kills_score() {
    let mut b = Bench::new("ffa");
    let (a, v) = duel(&mut b, 2); // Hunter: sniper
    let out = fire_right(&mut b, a);
    let shot = one(&out, "2");
    assert_eq!(shot.event.args[0]["i"], json!(a));
    let out = b.run(1000.0);
    let hit = one(&out, "1");
    assert_eq!(hit.event.args[0]["gID"], json!(v));
    assert_eq!(hit.event.args[0]["dID"], json!(a));
    let dmg = b.data.weapons[2].spec.dmg;
    assert!((hit.event.args[0]["healthDelta"].as_f64().unwrap() + dmg).abs() < 1e-9);

    // A 100 damage sniper shot kills a 100 health Triggerman outright.
    assert!(b.p(v).dead);
    let kill = &named(&out, "3")[0].event.args[0];
    assert_eq!(
        (kill["dID"].clone(), kill["gID"].clone()),
        (json!(a), json!(v))
    );
    assert_eq!(kill["sS"], json!(100.0));
    assert_eq!(one(&out, "5").event.args[0], json!("Ann killed Vic"));
    assert!((b.p(a).score - 100.0).abs() < 1e-9);
    assert_eq!((b.p(a).kills, b.p(v).deaths), (1, 1));
}

#[test]
fn assists_share_the_kill() {
    let mut b = Bench::new("ffa");
    let (a, v) = duel(&mut b, 2);
    let helper = b.join();
    b.spawn(helper, "Hal", 0);
    b.take();
    // The helper already dealt 40 of the victim's 100 health.
    b.p_mut(v).health = 60.0;
    b.p_mut(v).damage_sources.insert(helper, 40.0);
    fire_right(&mut b, a);
    let out = b.run(1000.0);
    let scores: Vec<&Value> = named(&out, "3").iter().map(|o| &o.event.args[0]).collect();
    let assist = scores.iter().find(|s| s["ast"] == json!(true)).unwrap();
    assert_eq!(
        (assist["dID"].clone(), assist["sS"].clone()),
        (json!(helper), json!(40.0))
    );
    let kill = scores.iter().find(|s| s["ast"].is_null()).unwrap();
    assert_eq!(kill["sS"], json!(60.0));
}

#[test]
fn reaching_the_score_limit_ends_the_round_and_starts_the_next() {
    let mut b = Bench::new("ffa");
    let (a, v) = duel(&mut b, 2);
    b.p_mut(a).score = 1450.0;
    fire_right(&mut b, a);
    let out = b.run(1000.0);
    let end = one(&out, "7");
    assert!(b.room.round_end);
    assert_eq!(
        end.event.args[1].as_array().unwrap().len(),
        b.data.modes.len()
    );
    b.send(v, "modeVote", vec![json!(2)]);
    let out = b.run(f64::from(b.rules.rules.round_end_countdown_s + 2) * 1000.0);
    let counts: Vec<i64> = named(&out, "8")
        .iter()
        .map(|o| o.event.args[0].as_i64().unwrap())
        .collect();
    assert_eq!(
        counts.first(),
        Some(&i64::from(b.rules.rules.round_end_countdown_s))
    );
    assert_eq!(counts.last(), Some(&0));
    // The vote picked the next mode; everyone gets their own welcome.
    assert_eq!(b.room.mode_index, 2);
    assert!(!b.room.round_end);
    let welcomes = named(&out, "welcome");
    assert_eq!(welcomes.len(), 2);
    for w in welcomes {
        let To::One(to) = w.to else { panic!() };
        assert_eq!(w.event.args[0]["id"], json!(to));
    }
    assert!(b.room.players.iter().all(|p| p.score == 0.0 && p.dead));
}

#[test]
fn team_scores_are_sent_raw() {
    let mut b = Bench::new("tdm");
    let (a, _) = duel(&mut b, 2);
    let team = b.p(a).team.clone();
    // Teams alternate, so the duel is red against blue.
    assert_ne!(team, b.room.players[1].team);
    fire_right(&mut b, a);
    let out = b.run(1000.0);
    let ts = named(&out, "ts").last().unwrap().event.args.clone();
    let mine = if team == "red" { &ts[0] } else { &ts[1] };
    assert_eq!(*mine, json!(100.0));
}

#[test]
fn rockets_explode_and_hurt_the_shooter_too() {
    let mut b = Bench::new("ffa");
    let (a, v) = duel(&mut b, 5); // Rocketeer
    // Point blank: the victim stands right in front.
    let ax = b.p(a).x;
    b.p_mut(v).x = ax + 120.0;
    fire_right(&mut b, a);
    let out = b.run(2000.0);
    assert!(!named(&out, "ex").is_empty());
    assert!(b.p(v).health < b.p(v).max_health);
}

#[test]
fn chat_is_cleaned_and_team_chat_stays_in_the_team() {
    let mut b = Bench::new("tdm");
    let (a, _) = duel(&mut b, 0);
    let long = "x".repeat(200);
    let out = b.send(
        a,
        "cht",
        vec![json!(format!("<i>hi</i> {long}")), json!("ALL")],
    );
    let msg = one(&out, "cht");
    assert_eq!(msg.to, To::All);
    let text = msg.event.args[0][1].as_str().unwrap();
    assert!(text.starts_with("hi x") && text.chars().count() == b.rules.rules.chat_max_len);
    let team = b.p(a).team.clone();
    let out = b.send(a, "cht", vec![json!("push"), json!("TEAM")]);
    let msg = one(&out, "cht");
    assert_eq!(msg.to, To::Team(team));
    assert_eq!(msg.event.args[0][1], json!("(TEAM) push"));
    assert!(b.send(a, "cht", vec![json!("<b></b>")]).is_empty());
}

#[test]
fn mode_votes_move_with_the_voter() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    b.take();
    let out = b.send(i, "modeVote", vec![json!(3)]);
    assert_eq!(
        one(&out, "vt").event.args[0],
        json!({"i": 3, "n": "Lootcrate", "v": 1})
    );
    let out = b.send(i, "modeVote", vec![json!(1)]);
    let vt: Vec<&Value> = named(&out, "vt").iter().map(|o| &o.event.args[0]).collect();
    assert_eq!(vt[0]["v"], json!(0));
    assert_eq!(vt[1]["i"], json!(1));
    assert!(b.send(i, "modeVote", vec![json!(99)]).is_empty());
    assert!(b.send(i, "modeVote", vec![json!("2")]).is_empty());
}

#[test]
fn custom_server_settings_are_clamped() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    b.send(
        i,
        "cSrv",
        vec![json!({
            "srvPlayers": "999",
            "srvHealthMult": "1e9",
            "srvSpeedMult": -4,
            "srvModes": [4, 77],
        })],
    );
    assert_eq!(b.room.max_players, 8);
    assert_eq!(b.room.mode(&b.data).code, "snipe");
    b.spawn(i, "x", 0);
    let p = b.p(i);
    assert!(
        (p.max_health - 50.0 * 100.0).abs() < 1e-9,
        "{}",
        p.max_health
    );
    assert!(p.speed > 0.0);
}

#[test]
fn hardpoints_score_on_server_time_without_input() {
    let mut b = Bench::new("hp");
    let i = b.join();
    b.spawn(i, "x", 0);
    let team = b.p(i).team.clone();
    let tiles = b.room.world.score_tiles.clone();
    let Some(&point) = tiles
        .iter()
        .find(|&&k| b.room.world.tiles[k].obj_team != team)
    else {
        panic!("the arena has a hardpoint");
    };
    let tl = &b.room.world.tiles[point];
    let centre = (tl.x + tl.scale / 2.0, tl.y + tl.scale / 2.0);
    let p = b.p_mut(i);
    (p.x, p.y) = centre;
    // A hidden tab sends no input; standing on the point still scores,
    // once at once and once per interval (plus the tick that notices it).
    let interval = b.rules.rules.hardpoint_interval_ms;
    b.run(interval * 3.0 + 200.0);
    let points = f64::from(b.rules.rules.hardpoint_points);
    assert!(
        (b.p(i).score - points * 4.0).abs() < 1e-9,
        "score {}",
        b.p(i).score
    );
}

#[test]
fn healthpacks_heal_and_come_back() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    b.spawn(i, "x", 0);
    let Some(k) = b
        .room
        .world
        .pickups
        .iter()
        .position(|p| p.kind == "healthpack")
    else {
        panic!("the arena has healthpacks");
    };
    let (px, py) = (b.room.world.pickups[k].x, b.room.world.pickups[k].y);
    let p = b.p_mut(i);
    p.x = px;
    p.y = py;
    p.health = 10.0;
    // Picked up on the next server tick, without any input.
    let out = b.run(20.0);
    assert!((b.p(i).health - 100.0).abs() < 1e-9);
    let gone = named(&out, "4");
    assert_eq!(gone[0].event.args[0]["active"], json!(false));
    let back = b.run(b.rules.rules.healthpack_respawn_ms + 100.0);
    assert!(
        named(&back, "4")
            .iter()
            .any(|o| o.event.args[0]["active"] == json!(true))
    );
}

#[test]
fn leaving_removes_the_player() {
    let mut b = Bench::new("ffa");
    let (a, v) = duel(&mut b, 0);
    b.room.leave(&b.data, &b.rules.rules, v, b.now);
    let out = b.take();
    assert_eq!(one(&out, "rem").event.args[0], json!(v));
    assert_eq!(b.room.players.len(), 1);
    assert_eq!(b.room.players[0].index, a);
}

#[test]
fn early_balance_versions_hide_later_classes() {
    let mut b = Bench::with_balance("ffa", "v0.20");
    let i = b.join();
    let late = b.data.classes.iter().position(|c| !c.available);
    if let Some(late) = late {
        b.spawn(i, "x", late);
        assert_eq!(b.p(i).class_index, 0, "falls back to the first class");
    }
}

// The whole path: Socket.IO namespace per room, as KRP's client connects.

fn query(sid: Option<&str>) -> Query {
    Query {
        eio: Some("4".into()),
        transport: Some("polling".into()),
        sid: sid.map(str::to_owned),
    }
}

fn packets(body: &[u8]) -> Vec<Packet> {
    decode_payload(std::str::from_utf8(body).unwrap()).unwrap()
}

#[tokio::test]
async fn clients_join_rooms_by_namespace() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let server = Server::new(Timing::default(), tx, Trace::disabled());
    let rules = assumptions::committed().0;
    let rooms = [
        RoomSpec {
            name: "DEV0".into(),
            mode: "ffa".into(),
            max_players: None,
        },
        RoomSpec {
            name: "DEV1".into(),
            mode: "tdm".into(),
            max_players: None,
        },
    ];
    let mut game = Game::new(rules, committed("best"), arena(), &rooms, Trace::disabled()).unwrap();
    assert_eq!(game.resolve_room("DEV1").as_deref(), Some("DEV1"));
    assert_eq!(game.resolve_room("").as_deref(), Some("DEV0"));
    let list = game.room_list();
    assert_eq!(list[1]["m"], json!("tdm"));
    assert_eq!(list[0]["mxpl"], json!(8));

    let open = packets(&server.get(&query(None), "h").await.body);
    let sid = serde_json::from_str::<Value>(&open[0].data).unwrap()["sid"]
        .as_str()
        .unwrap()
        .to_owned();
    server.post(&query(Some(&sid)), b"40/DEV1,");
    game.handle(rx.recv().await.unwrap());
    assert_eq!(game.room_list()[1]["pl"], json!(1));
    let got = packets(&server.get(&query(Some(&sid)), "h").await.body);
    assert!(got[0].data.starts_with("0/DEV1,{\"sid\":"));
    assert_eq!(got[1].data, "2/DEV1,[\"yourRoom\",\"DEV1\"]");
    assert!(got[2].data.starts_with("2/DEV1,[\"welcome\","));

    server.post(&query(Some(&sid)), b"42/DEV1,[\"ping1\"]");
    game.handle(rx.recv().await.unwrap());
    let got = packets(&server.get(&query(Some(&sid)), "h").await.body);
    assert_eq!(got[0].data, "2/DEV1,[\"pong1\"]");

    // Unknown rooms are refused rather than created.
    server.post(&query(Some(&sid)), b"40/elsewhere,");
    game.handle(rx.recv().await.unwrap());
    let got = packets(&server.get(&query(Some(&sid)), "h").await.body);
    assert!(got[0].data.starts_with("4/elsewhere,"));

    server.post(&query(Some(&sid)), b"41/DEV1,");
    game.handle(rx.recv().await.unwrap());
    assert_eq!(game.room_list()[1]["pl"], json!(0));
}

#[test]
fn configured_rooms_are_checked() {
    let bad = |name: &str, mode: &str| {
        let spec = [RoomSpec {
            name: name.into(),
            mode: mode.into(),
            max_players: None,
        }];
        Game::new(
            assumptions::committed().0,
            committed("best"),
            arena(),
            &spec,
            Trace::disabled(),
        )
        .is_err()
    };
    assert!(bad("DEV0", "nope"));
    assert!(bad("a/b", "ffa"));
    assert!(!bad("DEV0", "ffa"));
}

// The 2016 client's path: Engine.IO 3 on the root namespace, seated by
// `create`, with events translated both ways.

mod classic {
    use super::*;
    use crate::classic::codec3::{Packet as Packet3, PacketType, decode_payload_string};
    use crate::classic::eio3;

    const CONTRACT: &str = include_str!("../../data/contracts/20160806061006.json");

    /// Server-to-client argument counts the 2016 client's handlers take.
    fn contract() -> HashMap<String, Vec<usize>> {
        let doc: Value = serde_json::from_str(CONTRACT).unwrap();
        doc["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["direction"] == "server->client")
            .map(|e| {
                let counts = e["counts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| usize::try_from(c.as_u64().unwrap()).unwrap())
                    .collect();
                (e["event"].as_str().unwrap().to_owned(), counts)
            })
            .collect()
    }

    fn query(sid: Option<&str>) -> eio3::Query {
        eio3::Query {
            eio: Some("3".into()),
            transport: Some("polling".into()),
            sid: sid.map(str::to_owned),
            b64: Some("1".into()),
            j: None,
        }
    }

    struct Client {
        sid: String,
    }

    impl Client {
        async fn open(server: &eio3::Server) -> Self {
            let r = server.get(&query(None), "h").await;
            let packets = decode_payload_string(std::str::from_utf8(&r.body).unwrap()).unwrap();
            let open: Value = serde_json::from_str(&packets[0].data).unwrap();
            Self {
                sid: open["sid"].as_str().unwrap().to_owned(),
            }
        }

        fn send(&self, server: &eio3::Server, name: &str, args: &[Value]) {
            let mut arr = vec![json!(name)];
            arr.extend_from_slice(args);
            let msg = format!("42{}", Value::Array(arr));
            let body = format!("{}:{msg}", msg.chars().count());
            assert_eq!(
                server.post(&query(Some(&self.sid)), body.as_bytes()).status,
                200
            );
        }

        /// Every event queued for this client, checked against the contract.
        async fn events(&self, server: &eio3::Server) -> Vec<(String, Vec<Value>)> {
            let r = server.get(&query(Some(&self.sid)), "h").await;
            let packets: Vec<Packet3> =
                decode_payload_string(std::str::from_utf8(&r.body).unwrap()).unwrap();
            let contract = contract();
            let mut out = Vec::new();
            for p in packets {
                if p.kind != PacketType::Message || !p.data.starts_with('2') {
                    continue;
                }
                let arr: Vec<Value> = serde_json::from_str(&p.data[1..]).unwrap();
                let name = arr[0].as_str().unwrap().to_owned();
                let args = arr[1..].to_vec();
                if let Some(counts) = contract.get(&name) {
                    assert!(
                        counts.contains(&args.len()),
                        "{name} sent with {} arguments, the client takes {counts:?}",
                        args.len()
                    );
                }
                out.push((name, args));
            }
            out
        }
    }

    async fn pump(game: &mut Game, rx: &mut mpsc::UnboundedReceiver<eio3::TransportEvent>) {
        while let Ok(ev) = rx.try_recv() {
            game.handle_classic(ev);
        }
        tokio::task::yield_now().await;
    }

    fn find<'a>(evs: &'a [(String, Vec<Value>)], name: &str) -> &'a [Value] {
        &evs.iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| {
                panic!(
                    "no {name} in {:?}",
                    evs.iter().map(|e| &e.0).collect::<Vec<_>>()
                )
            })
            .1
    }

    #[tokio::test]
    async fn the_2016_client_joins_and_plays() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let server = eio3::Server::new(eio3::Timing::default(), tx, Trace::disabled());
        let rooms = [RoomSpec {
            name: "DEV0".into(),
            mode: "ffa".into(),
            max_players: None,
        }];
        let mut game = Game::new(
            assumptions::committed().0,
            committed("best"),
            arena(),
            &rooms,
            Trace::disabled(),
        )
        .unwrap();
        assert!(game.set_classic_room("nope").is_err());

        let a = Client::open(&server).await;
        let b = Client::open(&server).await;
        pump(&mut game, &mut rx).await;
        assert_eq!(game.room_list()[0]["pl"], json!(0), "no seat before create");

        a.send(&server, "create", &[]);
        a.send(&server, "respawn", &[]);
        pump(&mut game, &mut rx).await;
        let evs = a.events(&server).await;
        assert_eq!(find(&evs, "yourRoom"), [json!("DEV0"), json!("h/DEV0")]);
        let welcomes: Vec<_> = evs.iter().filter(|(n, _)| n == "welcome").collect();
        assert_eq!(welcomes.len(), 1, "only the respawn's welcome");
        assert_eq!(welcomes[0].1[1], json!(false));
        let mut obj = welcomes[0].1[0].clone();
        obj["name"] = json!("Alpha");
        obj["classIndex"] = json!(0);

        a.send(
            &server,
            "gotit",
            &[obj, json!(false), json!(0), json!(false)],
        );
        pump(&mut game, &mut rx).await;
        let evs = a.events(&server).await;
        let setup = find(&evs, "gameSetup");
        assert_eq!(setup[1], json!(true), "the map comes with the first setup");
        let doc: Value = serde_json::from_str(setup[0].as_str().unwrap()).unwrap();
        assert_eq!(doc["you"]["name"], json!("Alpha"));
        assert!(doc["you"]["spawnProtection"].is_number());
        let me = doc["you"]["index"].clone();

        // A second 2016 client sees the first; the first sees it arrive.
        b.send(&server, "create", &[json!("somelobby")]);
        b.send(&server, "respawn", &[]);
        pump(&mut game, &mut rx).await;
        let mut obj = find(&b.events(&server).await, "welcome")[0].clone();
        obj["name"] = json!("Bravo");
        obj["classIndex"] = json!(2);
        b.send(
            &server,
            "gotit",
            &[obj, json!(false), json!(0), json!(false)],
        );
        pump(&mut game, &mut rx).await;
        let evs = a.events(&server).await;
        let add: Value = serde_json::from_str(find(&evs, "add")[0].as_str().unwrap()).unwrap();
        assert_eq!(add["name"], json!("Bravo"));
        assert_eq!(game.room_list()[0]["pl"], json!(2));

        // Input carries a timestamp, not a delta; likes name only the target.
        a.send(
            &server,
            "4",
            &[json!({"hdt": 1, "vdt": 0, "ts": 1000, "isn": 1, "s": 0})],
        );
        a.send(
            &server,
            "4",
            &[json!({"hdt": 1, "vdt": 0, "ts": 1016, "isn": 2, "s": 0})],
        );
        a.send(&server, "like", &[add["index"].clone()]);
        a.send(&server, "cht", &[json!("hi"), json!("ALL")]);
        pump(&mut game, &mut rx).await;
        game.tick(16.0);
        assert_eq!(find(&b.events(&server).await, "cht")[0], json!([me, "hi"]));
        let evs = a.events(&server).await;
        let x = doc["you"]["x"].as_f64().unwrap();
        let (_, rsd) = evs.iter().rev().find(|(n, _)| n == "rsd").unwrap();
        let rsd = rsd[0].as_array().unwrap();
        let mine = rsd.chunks(6).find(|c| c[1] == me).unwrap();
        assert!(mine[2].as_f64().unwrap() > x, "{mine:?}");
        assert_eq!(mine[5], json!(2), "echoes the input number");

        // Leaving frees the seat.
        assert_eq!(server.post(&query(Some(&b.sid)), b"1:1").status, 200);
        pump(&mut game, &mut rx).await;
        assert_eq!(game.room_list()[0]["pl"], json!(1));
        assert_eq!(find(&a.events(&server).await, "rem")[0], add["index"]);
    }
}

#[test]
fn player_limit_is_a_server_setting() {
    let spec = |max: Option<usize>| {
        [RoomSpec {
            name: "DEV0".into(),
            mode: "ffa".into(),
            max_players: max,
        }]
    };
    let game = |max| {
        Game::new(
            assumptions::committed().0,
            committed("best"),
            arena(),
            &spec(max),
            Trace::disabled(),
        )
    };
    assert!(game(Some(0)).is_err());
    assert!(game(Some(MAX_PLAYER_LIMIT + 1)).is_err());
    assert_eq!(game(None).unwrap().room_list()[0]["mxpl"], json!(8));
    assert_eq!(game(Some(12)).unwrap().room_list()[0]["mxpl"], json!(12));

    // The custom server form may lower the limit, never raise it.
    let mut b = Bench::new("ffa");
    b.room.set_player_limit(12);
    let i = b.join();
    let csrv = |n: &str| vec![json!({"srvPlayers": n})];
    b.send(i, "cSrv", csrv("10"));
    assert_eq!(b.room.max_players, 10);
    b.send(i, "cSrv", csrv("99"));
    assert_eq!(b.room.max_players, 12);
    b.send(i, "cSrv", csrv("1"));
    assert_eq!(b.room.max_players, 2);
}

#[test]
fn weapon_camos_survive_spawning_and_class_changes() {
    let mut b = Bench::new("ffa");
    let i = b.join();
    let camo_of = |b: &Bench, weapon: usize| {
        let p = b.p(i);
        let slot = p.weapon_ids.iter().position(|&w| w == weapon).unwrap();
        p.weapons[slot]["camo"].clone()
    };
    // A weapon two classes share (KRP: classes 1 and 4 both carry 5).
    let shared = b.data.classes[1].weapon_indexes[1];
    let other = (0..b.data.classes.len())
        .find(|&c| c != 1 && b.data.classes[c].weapon_indexes.contains(&shared))
        .unwrap();
    // KRP's client sends its saved camos as soon as it connects, before it
    // spawns, and again only when the loadout changes.
    b.send(i, "cCamo", vec![json!({"weaponID": shared, "camoID": 5})]);
    b.spawn(i, "a", 1);
    assert_eq!(camo_of(&b, shared), json!(4.0));
    // Respawning (a new `gotit`) keeps it.
    b.spawn(i, "a", 1);
    assert_eq!(camo_of(&b, shared), json!(4.0));
    // Another class with the same weapon wears the same camo.
    b.spawn(i, "a", other);
    assert_eq!(camo_of(&b, shared), json!(4.0));
    // Camo 0 takes it off.
    b.send(i, "cCamo", vec![json!({"weaponID": shared, "camoID": 0})]);
    assert_eq!(camo_of(&b, shared), json!(-1.0));
    b.spawn(i, "a", 1);
    assert_eq!(camo_of(&b, shared), json!(-1.0));
}

#[test]
fn other_players_see_a_spray_change() {
    let mut b = Bench::new("ffa");
    // A spray added from the sprays folder.
    b.data.cosmetics.sprays.push(
        json!({"id": 84, "name": "Added", "info": {"scale": 64, "alpha": 1, "resolution": 30}}),
    );
    let (a, c) = (b.join(), b.join());
    b.spawn(c, "c", 1);
    let spray_in = |out: &[Out]| -> Value {
        let add = one(out, "add");
        let s: Value = serde_json::from_str(add.event.args[0].as_str().unwrap()).unwrap();
        s["spray"].clone()
    };
    // Chosen before spawning: the spawn announces it.
    b.send(a, "cSpray", vec![json!(84)]);
    let out = b.spawn(a, "a", 1);
    assert_eq!(spray_in(&out)["src"], json!("/assets/sprays/84.png"));
    // Changed while alive: everyone hears it at once, not at the next spawn.
    let out = b.send(a, "cSpray", vec![json!(2)]);
    assert!(matches!(one(&out, "add").to, To::All));
    assert_eq!(spray_in(&out)["id"], json!(2));
    // While dead nothing is sent; the next spawn carries it.
    b.p_mut(a).dead = true;
    assert!(named(&b.send(a, "cSpray", vec![json!(3)]), "add").is_empty());
}

// The admin console.
mod admin;

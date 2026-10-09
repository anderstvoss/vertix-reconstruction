//! Game logic against the 2016-08-06 contract, without HTTP.

use super::*;
use crate::eio::codec;
use crate::eio::{Query, Server, Timing};
use serde_json::Value;

const ARENA: &str = include_str!("../../data/maps/arena.txt");
const CONTRACTS: &str = include_str!("../../data/contracts/20160806061006.json");

fn rules() -> Assumptions {
    Assumptions::parse(include_str!("../../data/assumptions.toml")).unwrap()
}

struct Harness {
    server: Arc<Server>,
    rx: mpsc::UnboundedReceiver<TransportEvent>,
    game: Game,
}

struct Client {
    conn: ConnId,
    sid: String,
}

use std::sync::Arc;

impl Harness {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let r = rules();
        let map = Map::parse(ARENA, r.world.tile_scale, false).unwrap();
        Self {
            server,
            rx,
            game: Game::new(r, map, Trace::disabled()),
        }
    }

    fn query(sid: Option<&str>) -> Query {
        Query {
            eio: Some("3".into()),
            transport: Some("polling".into()),
            sid: sid.map(str::to_owned),
            b64: Some("1".into()),
            j: None,
        }
    }

    async fn connect(&mut self) -> Client {
        let r = self.server.get(&Self::query(None), "example.org").await;
        let packets = codec::decode_payload_string(std::str::from_utf8(&r.body).unwrap()).unwrap();
        let open: Value = serde_json::from_str(&packets[0].data).unwrap();
        let sid = open["sid"].as_str().unwrap().to_owned();
        let ev = self.rx.recv().await.unwrap();
        let TransportEvent::Connected { conn, .. } = &ev else {
            panic!("expected connect")
        };
        let conn = *conn;
        self.game.handle(ev);
        Client { conn, sid }
    }

    fn send(&mut self, c: &Client, name: &str, args: Vec<Value>) {
        self.game.handle(TransportEvent::Event {
            conn: c.conn,
            event: Event::new(name, args),
        });
    }

    /// Everything queued for `c`, as Socket.IO events.
    async fn drain(&mut self, c: &Client) -> Vec<Event> {
        let r = self
            .server
            .get(&Self::query(Some(&c.sid)), "example.org")
            .await;
        codec::decode_payload_string(std::str::from_utf8(&r.body).unwrap())
            .unwrap()
            .into_iter()
            .filter_map(|p| match crate::sio::decode(&p.data) {
                crate::sio::Incoming::Event(e) => Some(e),
                _ => None,
            })
            .collect()
    }

    /// Plays the client's side of a join: create, respawn, then gotit
    /// echoing the welcome object with a name and class.
    async fn join(&mut self, c: &Client, name: &str, class: u64) -> Vec<Event> {
        self.send(c, "create", vec![]);
        self.send(c, "respawn", vec![]);
        let evs = self.drain(c).await;
        let welcome = evs.iter().find(|e| e.name == "welcome").expect("welcome");
        let mut obj = welcome.args[0].clone();
        obj["name"] = json!(name);
        obj["classIndex"] = json!(class);
        self.send(
            c,
            "gotit",
            vec![obj, welcome.args[1].clone(), json!(0), json!(false)],
        );
        let mut all = evs;
        all.extend(self.drain(c).await);
        all
    }
}

fn find<'a>(evs: &'a [Event], name: &str) -> &'a Event {
    evs.iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no {name} in {evs:?}"))
}

#[tokio::test]
async fn join_sends_what_the_client_needs_to_start() {
    let mut h = Harness::new();
    let a = h.connect().await;
    let evs = h.join(&a, "<b>Ana</b>", 2).await;

    let room = find(&evs, "yourRoom");
    assert_eq!(room.args, vec![json!("1"), json!("example.org/1")]);
    let welcome = find(&evs, "welcome");
    assert_eq!(welcome.args[1], json!(false));
    assert!(welcome.args[0]["id"].is_number() && welcome.args[0]["room"] == json!("1"));

    let setup = find(&evs, "gameSetup");
    assert_eq!(setup.args[1], json!(true), "first setup carries the map");
    assert_eq!(setup.args[2], json!(true), "and starts the game");
    let s: Value = serde_json::from_str(setup.args[0].as_str().unwrap()).unwrap();
    for key in [
        "mapData",
        "maxScreenHeight",
        "maxScreenWidth",
        "tileScale",
        "usersInRoom",
        "viewMult",
        "you",
    ] {
        assert!(s.get(key).is_some(), "gameSetup lacks {key}");
    }
    let gd = &s["mapData"]["genData"];
    let (w, hgt) = (
        gd["width"].as_u64().unwrap(),
        gd["height"].as_u64().unwrap(),
    );
    assert_eq!(
        gd["data"]["data"].as_array().unwrap().len() as u64,
        w * hgt * 4
    );
    for key in ["name", "desc1", "desc2", "score", "teams", "code"] {
        assert!(
            s["mapData"]["gameMode"].get(key).is_some(),
            "gameMode lacks {key}"
        );
    }
    let you = &s["you"];
    assert_eq!(
        you["name"],
        json!("Ana"),
        "markup stripped like the client does"
    );
    assert_eq!(you["classIndex"], json!(2));
    assert_eq!(
        you["weapons"].as_array().unwrap().len(),
        2,
        "Hunter has two weapons"
    );
    assert_eq!(you["weapons"][0]["weaponIndex"], json!(2));
    assert_eq!(you["account"]["clan"], json!(""));
    assert_ne!(
        you["team"],
        json!(""),
        "an empty team hides the player from the stat table"
    );

    assert!(
        find(&evs, "lb").args[0]
            .as_array()
            .unwrap()
            .contains(&you["index"])
    );
    assert_eq!(find(&evs, "ts").args.len(), 2);
}

#[tokio::test]
async fn second_player_is_added_and_removed() {
    let mut h = Harness::new();
    let a = h.connect().await;
    h.join(&a, "a", 0).await;
    let b = h.connect().await;
    let evs_b = h.join(&b, "b", 1).await;
    let setup: Value =
        serde_json::from_str(find(&evs_b, "gameSetup").args[0].as_str().unwrap()).unwrap();
    assert_eq!(setup["usersInRoom"].as_array().unwrap().len(), 1);

    let evs_a = h.drain(&a).await;
    let add: Value = serde_json::from_str(find(&evs_a, "add").args[0].as_str().unwrap()).unwrap();
    assert_eq!(add["name"], json!("b"));

    h.game.handle(TransportEvent::Disconnected {
        conn: b.conn,
        reason: "test",
    });
    let evs_a = h.drain(&a).await;
    assert_eq!(find(&evs_a, "rem").args, vec![add["index"].clone()]);
}

#[tokio::test]
async fn movement_follows_the_clients_prediction() {
    let mut h = Harness::new();
    let a = h.connect().await;
    let evs = h.join(&a, "a", 0).await;
    let s: Value = serde_json::from_str(find(&evs, "gameSetup").args[0].as_str().unwrap()).unwrap();
    let (x0, y0) = (
        s["you"]["x"].as_f64().unwrap(),
        s["you"]["y"].as_f64().unwrap(),
    );
    let speed = s["you"]["speed"].as_f64().unwrap();
    // Two frames 16 ms apart moving right. The first input only sets the
    // clock; the client moved with its own frame delta, which the second
    // input's timestamp gives us.
    h.send(
        &a,
        "4",
        vec![json!({"hdt": 0.5, "vdt": 0, "ts": 1000, "isn": 0, "s": 0})],
    );
    h.send(
        &a,
        "4",
        vec![json!({"hdt": 0.5, "vdt": 0, "ts": 1016, "isn": 1, "s": 0})],
    );
    h.game.tick(16.0);
    let evs = h.drain(&a).await;
    let rsd = find(&evs, "rsd").args[0].as_array().unwrap().clone();
    assert_eq!(rsd[0], json!(6));
    assert_eq!(
        rsd[5],
        json!(1),
        "own record ends with the last input number"
    );
    let x1 = rsd[2].as_f64().unwrap();
    assert!(
        (x1 - (x0 + speed * 16.0).round()).abs() < 1.0,
        "x {x0} -> {x1}"
    );
    assert!((rsd[3].as_f64().unwrap() - y0).abs() < 1.0);
    // Stale or repeated input numbers are ignored.
    h.send(
        &a,
        "4",
        vec![json!({"hdt": 0.5, "vdt": 0, "ts": 5000, "isn": 1, "s": 0})],
    );
    h.game.tick(16.0);
    let evs = h.drain(&a).await;
    assert_eq!(find(&evs, "rsd").args[0][2], json!(x1));
}

#[tokio::test]
async fn ping_and_weapon_swap() {
    let mut h = Harness::new();
    let a = h.connect().await;
    h.join(&a, "a", 0).await;
    let b = h.connect().await;
    h.join(&b, "b", 0).await;
    let _ = h.drain(&a).await;
    h.send(&a, "ping1", vec![]);
    h.send(&a, "sw", vec![json!(1)]);
    h.send(&a, "sw", vec![json!(7)]);
    let evs = h.drain(&a).await;
    assert!(evs.iter().any(|e| e.name == "pong1" && e.args.is_empty()));
    let evs_b = h.drain(&b).await;
    let upd: Vec<_> = evs_b.iter().filter(|e| e.name == "upd").collect();
    assert_eq!(upd.len(), 1, "only the valid slot is relayed");
    assert_eq!(upd[0].args[0]["wi"], json!(1));
}

/// Every event the server sends must exist in the client's contract with
/// the same number of arguments.
#[tokio::test]
async fn emitted_events_match_the_contract() {
    let contracts: Value = serde_json::from_str(CONTRACTS).unwrap();
    let mut allowed: HashMap<String, Vec<u64>> = HashMap::new();
    for e in contracts["events"].as_array().unwrap() {
        if e["direction"] == json!("server->client") {
            allowed.insert(
                e["event"].as_str().unwrap().to_owned(),
                e["counts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c.as_u64().unwrap())
                    .collect(),
            );
        }
    }
    let mut h = Harness::new();
    let a = h.connect().await;
    let mut evs = h.join(&a, "a", 0).await;
    let b = h.connect().await;
    evs.extend(h.join(&b, "b", 4).await);
    h.send(
        &a,
        "4",
        vec![json!({"hdt": 0, "vdt": 0, "ts": 1, "isn": 0, "s": 1})],
    );
    h.send(&a, "sw", vec![json!(1)]);
    h.send(&b, "ftc", vec![json!(0)]);
    h.send(&b, "ping1", vec![]);
    h.game.tick(3000.0);
    evs.extend(h.drain(&a).await);
    evs.extend(h.drain(&b).await);
    h.game.handle(TransportEvent::Disconnected {
        conn: b.conn,
        reason: "test",
    });
    evs.extend(h.drain(&a).await);
    let mut seen = std::collections::BTreeSet::new();
    for e in &evs {
        let counts = allowed
            .get(&e.name)
            .unwrap_or_else(|| panic!("{} is not a server->client event of this build", e.name));
        assert!(
            counts.contains(&(e.args.len() as u64)),
            "{} sent with {} args",
            e.name,
            e.args.len()
        );
        seen.insert(e.name.clone());
    }
    for name in [
        "welcome",
        "yourRoom",
        "gameSetup",
        "add",
        "rem",
        "lb",
        "ts",
        "rsd",
        "upd",
        "jum",
        "pong1",
    ] {
        assert!(seen.contains(name), "{name} not exercised");
    }
}

#[test]
fn names_are_cleaned_like_the_client() {
    assert_eq!(clean_name("<i>x</i>y"), "xy");
    assert_eq!(clean_name("   "), DEFAULT_NAME);
    assert_eq!(clean_name(&"n".repeat(40)).len(), MAX_NAME);
}

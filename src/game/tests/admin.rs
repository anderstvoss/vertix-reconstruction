//! Admin console commands, on one room and through the game.

use super::*;

fn game(rooms: &[(&str, &str)]) -> Game {
    let specs: Vec<RoomSpec> = rooms
        .iter()
        .map(|(n, m)| RoomSpec {
            name: (*n).into(),
            mode: (*m).into(),
            max_players: None,
        })
        .collect();
    let mut g = Game::new(
        assumptions::committed().0,
        committed("best"),
        arena(),
        &specs,
        Trace::disabled(),
    )
    .unwrap();
    g.set_admin(
        None,
        super::super::admin::Paths {
            krp_data: concat!(env!("CARGO_MANIFEST_DIR"), "/data/krp").into(),
            balance_dir: concat!(env!("CARGO_MANIFEST_DIR"), "/data/balance").into(),
            rules: vec![
                concat!(env!("CARGO_MANIFEST_DIR"), "/data/rules/base.toml").into(),
                concat!(env!("CARGO_MANIFEST_DIR"), "/data/rules/recovered.toml").into(),
            ],
        },
    );
    g
}

/// Seats a player straight in a room, as if their client had connected.
fn seat(g: &mut Game, room: &str, name: &str) -> u32 {
    let data = &g.data;
    let s = g.rooms.iter_mut().find(|s| s.room.name == room).unwrap();
    let i = s.room.join(data);
    let ev = Event::new(
        "gotit",
        vec![json!({"name": name, "classIndex": 0}), json!(false)],
    );
    s.room.on_event(data, &g.maps, &g.rules.rules, i, &ev, 0.0);
    s.room.out.clear();
    i
}

fn room<'a>(g: &'a Game, name: &str) -> &'a Room {
    &g.rooms.iter().find(|s| s.room.name == name).unwrap().room
}

fn ok(g: &mut Game, line: &str) -> crate::admin::Reply {
    let r = g.run_command(line, None);
    assert!(r.ok, "{line}: {}", r.text);
    r
}

fn err(g: &mut Game, line: &str) -> String {
    let r = g.run_command(line, None);
    assert!(!r.ok, "{line} should fail: {}", r.text);
    r.text
}

#[test]
fn win_and_lose_end_the_round_with_the_right_winner() {
    let mut g = game(&[("A", "tdm")]);
    seat(&mut g, "A", "Bob");
    let team = room(&g, "A").players[0].team.clone();
    ok(&mut g, "lose Bob");
    assert!(room(&g, "A").round_end);
    let other = if team == "red" { "blue" } else { "red" };
    // A second end is refused while the countdown runs.
    assert!(err(&mut g, "win red").contains("already over"));
    ok(&mut g, "restart");
    assert!(!room(&g, "A").round_end);
    let r = ok(&mut g, &format!("win {other}"));
    assert!(r.text.contains(other));
}

#[test]
fn win_in_free_for_all_names_the_player() {
    let mut b = Bench::new("ffa");
    let a = b.join();
    b.spawn(a, "A", 0);
    let c = b.join();
    b.spawn(c, "C", 0);
    b.take();
    b.room
        .admin_add_score(&b.data, &b.rules.rules, c, 50.0, b.now);
    let w = b.room.admin_winner_against(&b.data, &a.to_string());
    assert_eq!(w, c.to_string());
    b.room.admin_end_round(&b.rules.rules, &w, b.now).unwrap();
    let out = b.take();
    assert_eq!(one(&out, "7").event.args[0], json!(c.to_string()));
    // The countdown then starts the next round, as after a normal win.
    let out = b.run(f64::from(b.rules.rules.round_end_countdown_s + 2) * 1000.0);
    assert!(!named(&out, "welcome").is_empty());
    assert!(!b.room.round_end);
}

#[test]
fn kill_all_slays_without_scoring() {
    let mut b = Bench::new("ffa");
    let a = b.join();
    b.spawn(a, "A", 0);
    let c = b.join();
    b.spawn(c, "C", 0);
    b.take();
    assert!(b.room.admin_slay(&b.data, &b.rules.rules, a, b.now));
    assert!(b.room.admin_slay(&b.data, &b.rules.rules, c, b.now));
    assert!(!b.room.admin_slay(&b.data, &b.rules.rules, c, b.now));
    let out = b.take();
    assert_eq!(named(&out, "3").len(), 2);
    for i in [a, c] {
        assert!(b.p(i).dead);
        assert_eq!(b.p(i).deaths, 1);
        assert!(b.p(i).score.abs() < f64::EPSILON);
    }
}

#[test]
fn mode_and_map_commands_start_new_rounds() {
    let mut g = game(&[("A", "ffa"), ("B", "tdm")]);
    seat(&mut g, "A", "Ann");
    ok(&mut g, "mode tdm arena");
    assert_eq!(room(&g, "A").mode(&g.data).code, "tdm");
    assert_eq!(room(&g, "A").map_id, "arena");
    ok(&mut g, "@B mode Hardpoint");
    assert_eq!(room(&g, "B").mode(&g.data).code, "hp");
    ok(&mut g, "map arena");
    assert!(err(&mut g, "map nowhere").contains("no map"));
    assert!(err(&mut g, "mode nope").contains("modes:"));
}

#[test]
fn scores_limits_and_settings() {
    let mut g = game(&[("A", "tdm")]);
    seat(&mut g, "A", "Ann");
    ok(&mut g, "scorelimit 30");
    ok(&mut g, "addscore red 10");
    assert!(!room(&g, "A").round_end);
    ok(&mut g, "score red 30");
    assert!(room(&g, "A").round_end);
    ok(&mut g, "scorelimit default");
    assert!(room(&g, "A").score_limit.is_none());
    ok(&mut g, "maxplayers 3");
    assert_eq!(room(&g, "A").max_players, 3);
    assert!(!g.run_command("maxplayers 0", None).ok);
    ok(&mut g, "healthmult 2");
    assert_eq!(room(&g, "A").admin_mults(), (2.0, 1.0));
}

#[test]
fn rules_change_for_every_room_and_reload() {
    let mut g = game(&[("A", "ffa")]);
    ok(&mut g, "rule round_end_countdown_s 3");
    assert_eq!(g.rules.rules.round_end_countdown_s, 3);
    assert!(err(&mut g, "rule round_end_countdown_s 2.5").contains("round_end_countdown_s"));
    assert!(err(&mut g, "rule nope 1").contains("no rule"));
    ok(&mut g, "reload rules");
    assert_eq!(g.rules.rules.round_end_countdown_s, 15);
    let r = ok(&mut g, "rule");
    assert!(r.data["chat_max_len"].is_number());
}

#[test]
fn balance_presets_switch() {
    let mut g = game(&[("A", "ffa")]);
    ok(&mut g, "balance krp");
    assert_eq!(g.data.balance.id, "krp");
    assert!(!g.run_command("balance ../x", None).ok);
    let r = ok(&mut g, "list presets");
    assert!(r.text.lines().any(|l| l == "best"));
}

#[test]
fn rooms_open_close_and_pause() {
    let mut g = game(&[("A", "ffa")]);
    ok(&mut g, "open Z zmtch");
    assert!(err(&mut g, "open Z ffa").contains("already open"));
    seat(&mut g, "Z", "Zed");
    ok(&mut g, "@Z pause");
    assert!(g.state_json()["rooms"][1]["paused"].as_bool().unwrap());
    ok(&mut g, "@Z resume");
    ok(&mut g, "classic Z");
    ok(&mut g, "close Z");
    assert_eq!(g.rooms.len(), 1);
    assert_eq!(g.classic_room, "A");
    assert!(err(&mut g, "close A").contains("last room"));
}

#[test]
fn players_are_found_by_index_name_or_prefix() {
    let mut g = game(&[("A", "ffa")]);
    let ann = seat(&mut g, "A", "Ann");
    seat(&mut g, "A", "Andy");
    let r = room(&g, "A");
    assert_eq!(r.find_player(&ann.to_string()), Ok(ann));
    assert_eq!(r.find_player("ann"), Ok(ann));
    assert!(r.find_player("an").unwrap_err().contains("2 players"));
    assert!(r.find_player("x").is_err());
    ok(&mut g, "health Andy 7");
    ok(&mut g, "protect Ann on");
    ok(&mut g, "rename Ann Annie");
    assert_eq!(room(&g, "A").players[0].name, "Annie");
    ok(&mut g, "tp Annie 100 200");
    assert!((room(&g, "A").players[0].x - 100.0).abs() < f64::EPSILON);
    ok(&mut g, r#"inject Annie cht ["hello"]"#);
    ok(&mut g, r#"emit all 6 ["Title", "text", 1.25]"#);
    ok(&mut g, "killall");
    assert!(room(&g, "A").players.iter().all(|p| p.dead));
    let r = ok(&mut g, "players");
    assert!(r.text.contains("Annie"));
    assert!(ok(&mut g, "status").data["rooms"][0]["players"].is_array());
    assert!(ok(&mut g, "catalog").data["commands"].is_array());
    assert!(err(&mut g, "frobnicate").contains("help"));
}

#[tokio::test]
async fn kick_tells_the_client_and_frees_the_seat() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let server = Server::new(Timing::default(), tx, Trace::disabled());
    let mut g = game(&[("DEV0", "ffa")]);
    let open = packets(&server.get(&query(None), "h").await.body);
    let sid = serde_json::from_str::<Value>(&open[0].data).unwrap()["sid"]
        .as_str()
        .unwrap()
        .to_owned();
    server.post(&query(Some(&sid)), b"40/DEV0,");
    g.handle(rx.recv().await.unwrap());
    let _ = server.get(&query(Some(&sid)), "h").await;
    assert_eq!(g.room_list()[0]["pl"], json!(1));
    ok(&mut g, "kick 0 Go away");
    assert_eq!(g.room_list()[0]["pl"], json!(0));
    assert!(g.conns.is_empty());
    let got = packets(&server.get(&query(Some(&sid)), "h").await.body);
    assert!(
        got.iter()
            .any(|p| p.data == "2/DEV0,[\"kick\",\"Go away\"]")
    );
    assert!(got.iter().any(|p| p.data == "1/DEV0,"));
}

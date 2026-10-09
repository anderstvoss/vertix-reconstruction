//! Event translation between the 2016-08-06 client and the KRP rooms.
//!
//! KRP's client descends from the 2019 live client, which descends from
//! the 2016 one, so most events are unchanged. The differences below were
//! RECOVERED from the 2016 client's handlers; anything not listed passes
//! through as it is.
//!
//! | Event | 2016 client | KRP room |
//! | --- | --- | --- |
//! | `4` in | `{hdt, vdt, ts, isn, s}`, no frame delta | `{hdt, vdt, delta, isn, s}` |
//! | `like` in | `like(target)` | `like(source, target)` |
//! | `1` out | `{dID, gID, dir, amount, bi, h}` | `{..., healthDelta, bulletIndex, health}` |
//! | `upd` out | `sp` is a number (it fires only at `0`), `l` a like count | `sp` a bool, `l` a list of likers |
//! | `ts` out | team modes: bar widths in percent; free for all: `(score limit, 0)` | raw team scores, or no arguments |
//! | `7` out | `(winner, players, votes, fading)` | `(winner, votes, fading)` |
//! | `tprt` out | `{indx, scor, newX, newY, oldX, oldY}` | `{indx, score, newX, newY}` |
//! | `yourRoom` out | `(room, server key)` | `(room)` |
//! | `gameSetup` out | the map only when it changed; `genData.data.data` | the map every time; `genData.data` |
//! | players | `spawnProtection` number, `likes` count | `isSpawnProtected`, `likedBy` |

use serde_json::{Map, Value, json};

use crate::game::data::GameData;
use crate::game::room::Room;
use crate::sio::Event;

/// Longest frame delta taken from the client's timestamps, in ms.
const MAX_INPUT_DELTA_MS: f64 = 100.0;

/// Per-client state the translation needs.
#[derive(Debug, Default, Clone)]
pub struct Member {
    /// Client timestamp of the last input, to derive its frame delta.
    pub last_ts: Option<f64>,
    /// Whether the client has the room's current map.
    pub has_map: bool,
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|f| f.is_finite())
}

/// A 2016 client event as the room expects it, or `None` to drop it.
#[must_use]
pub fn inbound(event: Event, index: u32, m: &mut Member) -> Option<Event> {
    match event.name.as_str() {
        "4" => {
            let input = event.args.first()?;
            let ts = num(input.get("ts"));
            let delta = match (m.last_ts, ts) {
                (Some(prev), Some(now)) => (now - prev).clamp(0.0, MAX_INPUT_DELTA_MS),
                _ => 0.0,
            };
            if ts.is_some() {
                m.last_ts = ts;
            }
            let mut out = input.as_object().cloned().unwrap_or_default();
            out.insert("delta".into(), json!(delta));
            Some(Event::new("4", vec![Value::Object(out)]))
        }
        "like" => {
            let target = event.args.first()?.clone();
            Some(Event::new("like", vec![json!(index), target]))
        }
        // Accounts, lobbies and the client's own cheat report have no
        // server side here; `create` is handled before the room.
        "create" | "kil" | "5" => None,
        n if n.starts_with("db") => None,
        _ => Some(event),
    }
}

/// A player object as the 2016 client stores it.
#[must_use]
pub fn player(mut p: Value) -> Value {
    if let Some(o) = p.as_object_mut() {
        let protected = o
            .get("isSpawnProtected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        o.insert("spawnProtection".into(), json!(i32::from(protected)));
        let likes = o
            .get("likedBy")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        o.insert("likes".into(), json!(likes));
        o.insert("loggedIn".into(), json!(false));
    }
    p
}

fn player_str(s: &Value) -> Option<Value> {
    let p: Value = serde_json::from_str(s.as_str()?).ok()?;
    Some(json!(player(p).to_string()))
}

/// What the room's leader stands at, for the 2016 score bar.
fn team_percent(score: f64, limit: f64) -> f64 {
    (score * 100.0 / limit.max(1.0)).clamp(0.0, 100.0).round()
}

/// A room event as the 2016 client expects it, or `None` to drop it.
#[must_use]
pub fn outbound(
    event: &Event,
    room: &Room,
    data: &GameData,
    host: &str,
    member: &mut Member,
) -> Option<Event> {
    let a = &event.args;
    let mut args = a.clone();
    match event.name.as_str() {
        "1" => {
            let src = a.first()?.as_object()?;
            let mut o = Map::new();
            for (k, v) in src {
                let k = match k.as_str() {
                    "healthDelta" => "amount",
                    "bulletIndex" => "bi",
                    "health" => "h",
                    other => other,
                };
                o.insert(k.to_owned(), v.clone());
            }
            args = vec![Value::Object(o)];
        }
        "upd" => {
            let mut o = a.first()?.as_object()?.clone();
            if let Some(sp) = o.get("sp").and_then(Value::as_bool) {
                o.insert("sp".into(), json!(i32::from(sp)));
            }
            if let Some(l) = o.get("l").and_then(Value::as_array) {
                let n = l.len();
                o.insert("l".into(), json!(n));
            }
            args = vec![Value::Object(o)];
        }
        "ts" => {
            let mode = room.mode(data);
            args = if mode.teams {
                vec![
                    json!(team_percent(room.score_red, mode.score)),
                    json!(team_percent(room.score_blue, mode.score)),
                ]
            } else {
                vec![json!(mode.score), json!(0)]
            };
        }
        "7" => {
            let users: Vec<Value> = room
                .players
                .iter()
                .map(|p| player(room.player_json(p)))
                .collect();
            args = vec![
                a.first().cloned().unwrap_or(Value::Null),
                Value::Array(users),
                a.get(1).cloned().unwrap_or(Value::Null),
                a.get(2).cloned().unwrap_or(json!(false)),
            ];
        }
        "tprt" => {
            let mut o = a.first()?.as_object()?.clone();
            if let Some(s) = o.remove("score") {
                o.insert("scor".into(), s);
            }
            // The old position is gone by now; a second puff at the new
            // one is the closest the client can show.
            let (x, y) = (o.get("newX").cloned(), o.get("newY").cloned());
            o.insert("oldX".into(), x.unwrap_or(Value::Null));
            o.insert("oldY".into(), y.unwrap_or(Value::Null));
            args = vec![Value::Object(o)];
        }
        "yourRoom" => {
            args.push(json!(format!("{host}/{}", room.name)));
        }
        // A welcome that opens the menu starts a new round, with a new map
        // the client must be sent.
        "welcome" if a.get(1).and_then(Value::as_bool) == Some(true) => {
            member.has_map = false;
        }
        "gameSetup" => {
            let mut doc: Value = serde_json::from_str(a.first()?.as_str()?).ok()?;
            // The 2016 client reads the pixels as an `ImageData` object.
            if let Some(gen_data) = doc.pointer_mut("/mapData/genData")
                && let Some(px) = gen_data.get_mut("data")
                && px.is_array()
            {
                *px = json!({"data": px.take()});
            }
            if let Some(you) = doc.get_mut("you") {
                *you = player(you.take());
            }
            if let Some(Value::Array(users)) = doc.get_mut("usersInRoom") {
                for u in users.iter_mut() {
                    *u = player(u.take());
                }
            }
            let with_map = !member.has_map;
            member.has_map = true;
            args = vec![json!(doc.to_string()), json!(with_map), json!(true)];
        }
        "add" => {
            args = vec![player_str(a.first()?)?];
        }
        // Shirts came after 2016; the client has no handler for them.
        "updShrt" => return None,
        _ => {}
    }
    Some(Event::new(event.name.clone(), args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_gets_a_frame_delta_from_timestamps() {
        let mut m = Member::default();
        let ev = |ts: f64| Event::new("4", vec![json!({"hdt": 1, "vdt": 0, "ts": ts, "isn": 1})]);
        let first = inbound(ev(1000.0), 0, &mut m).unwrap();
        assert_eq!(first.args[0]["delta"], json!(0.0));
        let next = inbound(ev(1033.0), 0, &mut m).unwrap();
        assert_eq!(next.args[0]["delta"], json!(33.0));
        let stalled = inbound(ev(9000.0), 0, &mut m).unwrap();
        assert_eq!(stalled.args[0]["delta"], json!(MAX_INPUT_DELTA_MS));
        assert_eq!(stalled.args[0]["hdt"], json!(1));
    }

    #[test]
    fn likes_name_the_sender_and_accounts_are_dropped() {
        let mut m = Member::default();
        let like = inbound(Event::new("like", vec![json!(4)]), 2, &mut m).unwrap();
        assert_eq!(like.args, vec![json!(2), json!(4)]);
        assert!(inbound(Event::new("dbLogin", vec![json!({})]), 2, &mut m).is_none());
        assert!(inbound(Event::new("create", vec![]), 2, &mut m).is_none());
        let fire = Event::new("1", vec![json!(1); 6]);
        assert_eq!(inbound(fire.clone(), 2, &mut m), Some(fire));
    }

    #[test]
    fn players_carry_the_2016_fields() {
        let p = player(json!({"isSpawnProtected": true, "likedBy": [1, 2]}));
        assert_eq!(p["spawnProtection"], json!(1));
        assert_eq!(p["likes"], json!(2));
        let p = player(json!({"isSpawnProtected": false}));
        assert_eq!(p["spawnProtection"], json!(0));
    }
}

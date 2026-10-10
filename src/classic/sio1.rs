//! Socket.IO 1.x packets, carried in Engine.IO message packets.
//!
//! The 2016 client only uses the default namespace, positional event
//! arguments and no acknowledgements, so that is all this handles. An
//! event is `2["name",arg,...]`; the server's connect is `0`.

use serde_json::Value;

pub use crate::sio::Event;

/// The Engine.IO message data for an event (`2[...]`), default namespace.
#[must_use]
pub fn encode(event: &Event) -> String {
    let mut arr = Vec::with_capacity(1 + event.args.len());
    arr.push(Value::String(event.name.clone()));
    arr.extend(event.args.iter().cloned());
    format!("2{}", Value::Array(arr))
}

/// What a client's Engine.IO message means at the Socket.IO level.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Connect,
    Disconnect,
    Event(Event),
    /// Anything else (acks, binary events, other namespaces): ignored.
    Unsupported,
}

/// The server's connect packet for the default namespace.
pub const CONNECT: &str = "0";

/// Parses the data of one Engine.IO message packet.
#[must_use]
pub fn decode(data: &str) -> Incoming {
    let mut chars = data.chars();
    match chars.next() {
        Some('0') => return Incoming::Connect,
        Some('1') => return Incoming::Disconnect,
        Some('2') => {}
        _ => return Incoming::Unsupported,
    }
    let rest = chars.as_str();
    // Default namespace only; skip an optional ack id.
    if rest.starts_with('/') {
        return Incoming::Unsupported;
    }
    let json = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    let Ok(Value::Array(mut arr)) = serde_json::from_str::<Value>(json) else {
        return Incoming::Unsupported;
    };
    if arr.is_empty() {
        return Incoming::Unsupported;
    }
    let Value::String(name) = arr.remove(0) else {
        return Incoming::Unsupported;
    };
    Incoming::Event(Event::new(name, arr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_events() {
        let e = Event::new(
            "gotit",
            vec![json!({"id": 3}), json!(false), json!(1), json!(false)],
        );
        let wire = encode(&e);
        assert_eq!(wire, r#"2["gotit",{"id":3},false,1,false]"#);
        assert_eq!(decode(&wire), Incoming::Event(e));
    }

    #[test]
    fn parses_control_packets() {
        assert_eq!(decode("0"), Incoming::Connect);
        assert_eq!(decode("1"), Incoming::Disconnect);
        assert_eq!(decode("3[]"), Incoming::Unsupported);
        assert_eq!(decode("2/admin,[\"x\"]"), Incoming::Unsupported);
        assert_eq!(decode("2[1]"), Incoming::Unsupported);
        assert_eq!(decode("2not json"), Incoming::Unsupported);
    }

    #[test]
    fn ignores_ack_ids() {
        assert_eq!(
            decode(r#"212["respawn"]"#),
            Incoming::Event(Event::new("respawn", vec![]))
        );
    }
}

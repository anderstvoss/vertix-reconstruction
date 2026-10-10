//! Socket.IO protocol 5 packets, carried in Engine.IO message packets.
//!
//! KRP's client opens one namespace per room (`io("/<room>")`), sends
//! events with positional arguments and never asks for acknowledgements,
//! so that is what this handles:
//!
//! * connect: `0/<ns>,{auth}` from the client, `0/<ns>,{"sid":"..."}` back;
//! * event: `2/<ns>,["name",arg,...]` (an ack id may sit before the `[`);
//! * disconnect: `1/<ns>,`;
//! * connect error: `4/<ns>,{"message":"..."}`.
//!
//! The default namespace `/` is written without the `/<ns>,` prefix.

use serde_json::Value;

/// A Socket.IO event: a name and its positional arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: String,
    pub args: Vec<Value>,
}

impl Event {
    #[must_use]
    pub fn new(name: impl Into<String>, args: Vec<Value>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// The Engine.IO message data for this event in namespace `ns`.
    #[must_use]
    pub fn encode(&self, ns: &str) -> String {
        let mut arr = Vec::with_capacity(1 + self.args.len());
        arr.push(Value::String(self.name.clone()));
        arr.extend(self.args.iter().cloned());
        format!("2{}{}", prefix(ns), Value::Array(arr))
    }
}

/// `/<ns>,` for a named namespace, nothing for `/`.
fn prefix(ns: &str) -> String {
    if ns == "/" {
        String::new()
    } else {
        format!("{ns},")
    }
}

/// The server's answer to a namespace connect.
#[must_use]
pub fn connect_ok(ns: &str, sid: &str) -> String {
    format!("0{}{}", prefix(ns), serde_json::json!({ "sid": sid }))
}

/// The server's refusal of a namespace connect.
#[must_use]
pub fn connect_error(ns: &str, message: &str) -> String {
    format!(
        "4{}{}",
        prefix(ns),
        serde_json::json!({ "message": message })
    )
}

/// The server closing one namespace socket.
#[must_use]
pub fn disconnect(ns: &str) -> String {
    format!("1{}", prefix(ns))
}

/// What a client's Engine.IO message means at the Socket.IO level.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Connect {
        ns: String,
    },
    Disconnect {
        ns: String,
    },
    Event {
        ns: String,
        event: Event,
    },
    /// Anything else (acks, binary events, malformed): ignored.
    Unsupported,
}

/// Parses the data of one Engine.IO message packet.
#[must_use]
pub fn decode(data: &str) -> Incoming {
    let mut chars = data.chars();
    let kind = chars.next();
    let rest = chars.as_str();
    let (ns, body) = if rest.starts_with('/') {
        match rest.split_once(',') {
            Some((ns, body)) => (ns.to_owned(), body),
            // `1/DEV0` with nothing after it is still a disconnect.
            None => (rest.to_owned(), ""),
        }
    } else {
        ("/".to_owned(), rest)
    };
    match kind {
        Some('0') => Incoming::Connect { ns },
        Some('1') => Incoming::Disconnect { ns },
        Some('2') => {
            let json = body.trim_start_matches(|c: char| c.is_ascii_digit());
            let Ok(Value::Array(mut arr)) = serde_json::from_str::<Value>(json) else {
                return Incoming::Unsupported;
            };
            if arr.is_empty() {
                return Incoming::Unsupported;
            }
            let Value::String(name) = arr.remove(0) else {
                return Incoming::Unsupported;
            };
            Incoming::Event {
                ns,
                event: Event { name, args: arr },
            }
        }
        _ => Incoming::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_events() {
        let e = Event::new("gotit", vec![json!({"id": 3}), json!(false), json!(1)]);
        let wire = e.encode("/DEV0");
        assert_eq!(wire, r#"2/DEV0,["gotit",{"id":3},false,1]"#);
        assert_eq!(
            decode(&wire),
            Incoming::Event {
                ns: "/DEV0".into(),
                event: e.clone()
            }
        );
        assert_eq!(e.encode("/"), r#"2["gotit",{"id":3},false,1]"#);
    }

    #[test]
    fn parses_control_packets() {
        assert_eq!(decode("0/DEV1,"), Incoming::Connect { ns: "/DEV1".into() });
        assert_eq!(
            decode("0/DEV1,{\"token\":1}"),
            Incoming::Connect { ns: "/DEV1".into() }
        );
        assert_eq!(decode("0"), Incoming::Connect { ns: "/".into() });
        assert_eq!(
            decode("1/DEV1,"),
            Incoming::Disconnect { ns: "/DEV1".into() }
        );
        assert_eq!(decode("3/DEV1,1[]"), Incoming::Unsupported);
        assert_eq!(decode("2/DEV1,[1]"), Incoming::Unsupported);
        assert_eq!(decode("2not json"), Incoming::Unsupported);
        assert_eq!(connect_ok("/DEV1", "abc"), r#"0/DEV1,{"sid":"abc"}"#);
        assert_eq!(
            connect_error("/x", "Invalid namespace"),
            r#"4/x,{"message":"Invalid namespace"}"#
        );
    }

    #[test]
    fn ignores_ack_ids() {
        assert_eq!(
            decode(r#"2/DEV0,12["respawn"]"#),
            Incoming::Event {
                ns: "/DEV0".into(),
                event: Event::new("respawn", vec![])
            }
        );
    }
}

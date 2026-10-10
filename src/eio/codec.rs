//! Engine.IO protocol 4 packets and long-polling payloads.
//!
//! This is the protocol the socket.io-client 4 in KRP's client speaks:
//!
//! * A packet is a type digit followed by its data (text only here; the
//!   game never sends binary).
//! * A polling payload is packets joined by the record separator `\x1e`.
//! * Over WebSocket, each frame is one packet.

use std::fmt;

/// Most packets accepted in one POST body. The client sends one input
/// packet per rendered frame and batches what it queued while a POST was
/// in flight; the HTTP body limit alone would allow thousands of empty
/// packets. NEW limit, not taken from any original server.
pub const MAX_PACKETS_PER_PAYLOAD: usize = 256;

/// Separates packets in a polling payload.
pub const SEPARATOR: char = '\u{1e}';

/// Engine.IO packet types (protocol 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Open,
    Close,
    Ping,
    Pong,
    Message,
    Upgrade,
    Noop,
}

impl PacketType {
    fn digit(self) -> char {
        match self {
            Self::Open => '0',
            Self::Close => '1',
            Self::Ping => '2',
            Self::Pong => '3',
            Self::Message => '4',
            Self::Upgrade => '5',
            Self::Noop => '6',
        }
    }

    fn from_digit(d: char) -> Option<Self> {
        Some(match d {
            '0' => Self::Open,
            '1' => Self::Close,
            '2' => Self::Ping,
            '3' => Self::Pong,
            '4' => Self::Message,
            '5' => Self::Upgrade,
            '6' => Self::Noop,
            _ => return None,
        })
    }
}

/// One Engine.IO packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub kind: PacketType,
    pub data: String,
}

impl Packet {
    #[must_use]
    pub fn new(kind: PacketType, data: impl Into<String>) -> Self {
        Self {
            kind,
            data: data.into(),
        }
    }

    /// A message packet (type 4) carrying a Socket.IO packet.
    #[must_use]
    pub fn message(data: impl Into<String>) -> Self {
        Self::new(PacketType::Message, data)
    }

    /// The packet as text: type digit, then data.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut s = String::with_capacity(1 + self.data.len());
        s.push(self.kind.digit());
        s.push_str(&self.data);
        s
    }

    /// Parses one packet.
    ///
    /// # Errors
    /// Fails on an empty string, an unknown type or a binary (`b`) packet.
    pub fn decode(text: &str) -> Result<Self, DecodeError> {
        let mut chars = text.chars();
        let d = chars.next().ok_or(DecodeError::Empty)?;
        let kind = PacketType::from_digit(d).ok_or(DecodeError::UnknownType(d))?;
        Ok(Self::new(kind, chars.as_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    UnknownType(char),
    TooManyPackets,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty packet"),
            Self::UnknownType(c) => write!(f, "unknown packet type {c:?}"),
            Self::TooManyPackets => {
                write!(
                    f,
                    "more than {MAX_PACKETS_PER_PAYLOAD} packets in one payload"
                )
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Joins packets into a polling payload.
#[must_use]
pub fn encode_payload(packets: &[Packet]) -> String {
    let mut out = String::new();
    for (i, p) in packets.iter().enumerate() {
        if i > 0 {
            out.push(SEPARATOR);
        }
        out.push_str(&p.encode());
    }
    out
}

/// Splits a polling payload into packets.
///
/// # Errors
/// Fails if any packet is malformed or there are too many.
pub fn decode_payload(body: &str) -> Result<Vec<Packet>, DecodeError> {
    let mut packets = Vec::new();
    for part in body.split(SEPARATOR) {
        if packets.len() >= MAX_PACKETS_PER_PAYLOAD {
            return Err(DecodeError::TooManyPackets);
        }
        packets.push(Packet::decode(part)?);
    }
    Ok(packets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_payloads() {
        let packets = vec![
            Packet::new(PacketType::Ping, ""),
            Packet::message("2/DEV0,[\"cht\",\"héllo\"]"),
        ];
        let wire = encode_payload(&packets);
        assert_eq!(wire, "2\u{1e}42/DEV0,[\"cht\",\"héllo\"]");
        assert_eq!(decode_payload(&wire).unwrap(), packets);
    }

    #[test]
    fn rejects_bad_payloads() {
        assert_eq!(decode_payload(""), Err(DecodeError::Empty));
        assert_eq!(decode_payload("9x"), Err(DecodeError::UnknownType('9')));
        assert_eq!(decode_payload("bAAAA"), Err(DecodeError::UnknownType('b')));
        let many = vec!["6"; MAX_PACKETS_PER_PAYLOAD + 1].join("\u{1e}");
        assert_eq!(decode_payload(&many), Err(DecodeError::TooManyPackets));
    }
}

//! Engine.IO protocol 3 packets and long-polling payloads.
//!
//! This follows the parser the archived Socket.IO 1.4.5 client bundles
//! (engine.io-parser 1.x), because that client is the one we must satisfy:
//!
//! * A packet is a type digit followed by its data. Text data is UTF-8
//!   encoded into a "binary string" (one char per byte) before framing, so
//!   lengths count UTF-8 bytes, not characters.
//! * A string payload is `<len>:<packet>` repeated. The client sends this
//!   form in POST bodies, as `text/plain;charset=UTF-8`, which means the
//!   byte-chars are UTF-8 encoded a second time on the wire.
//! * A binary payload is, per packet, `0x00` (string packet), the length as
//!   one byte per decimal digit, `0xFF`, then the packet bytes. The
//!   original server answered polls in this form unless the client asked
//!   for `b64=1`; the archived handshakes from 2016-01-18 are binary.

use std::fmt;

/// Engine.IO packet types (protocol 3).
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
    fn digit(self) -> u8 {
        match self {
            Self::Open => b'0',
            Self::Close => b'1',
            Self::Ping => b'2',
            Self::Pong => b'3',
            Self::Message => b'4',
            Self::Upgrade => b'5',
            Self::Noop => b'6',
        }
    }

    fn from_digit(d: u8) -> Option<Self> {
        Some(match d {
            b'0' => Self::Open,
            b'1' => Self::Close,
            b'2' => Self::Ping,
            b'3' => Self::Pong,
            b'4' => Self::Message,
            b'5' => Self::Upgrade,
            b'6' => Self::Noop,
            _ => return None,
        })
    }
}

/// One Engine.IO packet with text data. The client never sends binary
/// data, and the server has no reason to.
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

    #[must_use]
    pub fn message(data: impl Into<String>) -> Self {
        Self::new(PacketType::Message, data)
    }

    /// The packet as the UTF-8 bytes the parser frames.
    fn encoded(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.data.len());
        out.push(self.kind.digit());
        out.extend_from_slice(self.data.as_bytes());
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    BadLength,
    BadType,
    BadUtf8,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BadLength => "payload length prefix does not match its data",
            Self::BadType => "unknown packet type",
            Self::BadUtf8 => "packet data is not valid UTF-8",
        })
    }
}

impl std::error::Error for DecodeError {}

/// Encodes packets as a binary (XHR2) payload.
#[must_use]
pub fn encode_payload_binary(packets: &[Packet]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in packets {
        let bytes = p.encoded();
        out.push(0x00);
        out.extend(bytes.len().to_string().bytes().map(|d| d - b'0'));
        out.push(0xFF);
        out.extend_from_slice(&bytes);
    }
    out
}

/// Encodes packets as a string payload, the form sent when the client asked
/// for `b64=1`. The result is what goes on the wire as a UTF-8 body.
#[must_use]
pub fn encode_payload_string(packets: &[Packet]) -> String {
    if packets.is_empty() {
        return "0:".to_owned();
    }
    let mut out = String::new();
    for p in packets {
        let bytes = p.encoded();
        out.push_str(&bytes.len().to_string());
        out.push(':');
        out.extend(bytes.iter().map(|&b| char::from(b)));
    }
    out
}

/// Decodes a POST body sent by the client as a string payload.
///
/// # Errors
/// Returns an error if a length prefix is malformed or does not match, or
/// if a packet's type or text is invalid.
pub fn decode_payload_string(body: &str) -> Result<Vec<Packet>, DecodeError> {
    // Undo the wire-level UTF-8: every char must be one byte of the
    // parser's binary string.
    let raw: Vec<u8> = body
        .chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|_| DecodeError::BadUtf8))
        .collect::<Result<_, _>>()?;
    let mut packets = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let colon = raw[i..]
            .iter()
            .position(|&b| b == b':')
            .ok_or(DecodeError::BadLength)?;
        let digits = &raw[i..i + colon];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(DecodeError::BadLength);
        }
        let len: usize = std::str::from_utf8(digits)
            .map_err(|_| DecodeError::BadLength)?
            .parse()
            .map_err(|_| DecodeError::BadLength)?;
        let start = i + colon + 1;
        let end = start.checked_add(len).ok_or(DecodeError::BadLength)?;
        if end > raw.len() {
            return Err(DecodeError::BadLength);
        }
        if len > 0 {
            packets.push(decode_packet(&raw[start..end])?);
        }
        i = end;
    }
    Ok(packets)
}

fn decode_packet(bytes: &[u8]) -> Result<Packet, DecodeError> {
    let (&first, rest) = bytes.split_first().ok_or(DecodeError::BadType)?;
    let kind = PacketType::from_digit(first).ok_or(DecodeError::BadType)?;
    let data = std::str::from_utf8(rest).map_err(|_| DecodeError::BadUtf8)?;
    Ok(Packet::new(kind, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_payload_matches_archived_handshake_framing() {
        // The archived 2016-01-18 handshakes: 0x00, digits 9 7, 0xFF, then
        // a 97-byte open packet. Reproduce the framing for a 97-byte packet.
        let data = "x".repeat(96);
        let out = encode_payload_binary(&[Packet::new(PacketType::Open, data)]);
        assert_eq!(&out[..4], &[0x00, 9, 7, 0xFF]);
        assert_eq!(out[4], b'0');
        assert_eq!(out.len(), 4 + 97);
    }

    #[test]
    fn lengths_count_utf8_bytes() {
        let p = Packet::message("2[\"cht\",\"é\"]");
        let bin = encode_payload_binary(std::slice::from_ref(&p));
        // "4" + 12 chars, one of which is two bytes.
        assert_eq!(&bin[..4], &[0x00, 1, 4, 0xFF]);
        let s = encode_payload_string(std::slice::from_ref(&p));
        assert!(s.starts_with("14:4"));
        assert_eq!(decode_payload_string(&s).unwrap(), vec![p]);
    }

    #[test]
    fn decodes_several_client_packets() {
        let body = "1:211:42[\"ping1\"]";
        let got = decode_payload_string(body).unwrap();
        assert_eq!(
            got,
            vec![
                Packet::new(PacketType::Ping, ""),
                Packet::message("2[\"ping1\"]")
            ]
        );
        assert_eq!(
            got[1],
            decode_payload_string("11:42[\"ping1\"]").unwrap()[0]
        );
    }

    #[test]
    fn rejects_bad_lengths() {
        assert_eq!(decode_payload_string("5:42"), Err(DecodeError::BadLength));
        assert_eq!(decode_payload_string("x:4"), Err(DecodeError::BadLength));
        assert_eq!(decode_payload_string("2:9a"), Err(DecodeError::BadType));
        assert_eq!(
            decode_payload_string("2:4\u{263a}"),
            Err(DecodeError::BadUtf8)
        );
    }

    #[test]
    fn empty_string_payload() {
        assert_eq!(encode_payload_string(&[]), "0:");
        assert!(decode_payload_string("").unwrap().is_empty());
    }
}

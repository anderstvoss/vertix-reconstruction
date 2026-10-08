//! Bounded Engine.IO v3 polling payloads and minimal Socket.IO v1 packet tags.
//!
//! Based on recovered 2016 Vertix client protocol observations in the private
//! vertix-research repository. This is *new* protocol implementation code,
//! NOT archived game source and NOT a complete network / JSON event server.
//!
//! Text frames prefix packet length in UTF-16 code units; XHR2 binary frames
//! prefix UTF-8 byte length with numeric digit bytes, ending at 0xff.

/// Per-message safety bound for untrusted polling payloads. New server policy.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Per-request safety bound for untrusted polling payloads. New server policy.
pub const MAX_PAYLOAD_BYTES: usize = 4 * MAX_FRAME_BYTES;
/// Maximum packets accepted in one polling POST. New server policy.
pub const MAX_PACKETS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    EmptyLength,
    InvalidLength,
    Truncated,
    TooLarge,
    TooManyPackets,
    UnexpectedFrameType,
    InvalidUtf8,
    InvalidSocketPacket,
}

/// The framing carries Socket.IO packets unchanged; JSON payload parsing is
/// deliberately reserved for the future typed event adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketPacket<'a> {
    EnginePing,
    EnginePong,
    EngineNoop,
    Connect,
    Disconnect,
    EventJsonArray(&'a str),
}

/// # Errors
/// Returns `InvalidSocketPacket` for an unsupported or malformed envelope.
pub fn inspect_packet(packet: &str) -> Result<SocketPacket<'_>, FrameError> {
    match packet {
        "2" => Ok(SocketPacket::EnginePing),
        "3" => Ok(SocketPacket::EnginePong),
        "6" => Ok(SocketPacket::EngineNoop),
        "40" => Ok(SocketPacket::Connect),
        "41" => Ok(SocketPacket::Disconnect),
        _ if packet.starts_with("42[") && packet.ends_with(']') => {
            Ok(SocketPacket::EventJsonArray(&packet[2..]))
        }
        _ => Err(FrameError::InvalidSocketPacket),
    }
}

/// Serialize a bounded list of complete Engine.IO 3 text packets.
/// # Errors
/// Returns `TooLarge` or `TooManyPackets` if the output exceeds the new safety bounds.
pub fn encode_text(packets: &[&str]) -> Result<Vec<u8>, FrameError> {
    if packets.len() > MAX_PACKETS {
        return Err(FrameError::TooManyPackets);
    }
    let mut bytes = Vec::new();
    for packet in packets {
        if packet.len() > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge);
        }
        let length = packet.encode_utf16().count();
        let prefix = format!("{length}:");
        if bytes.len() + prefix.len() + packet.len() > MAX_PAYLOAD_BYTES {
            return Err(FrameError::TooLarge);
        }
        bytes.extend_from_slice(prefix.as_bytes());
        bytes.extend_from_slice(packet.as_bytes());
    }
    Ok(bytes)
}

/// Parse text packets, carefully distinguishing UTF-16 lengths from UTF-8
/// byte positions. Reject partial surrogate pairs and trailing junk.
/// # Errors
/// Returns a framing error for malformed lengths, partial UTF-16 code units,
/// invalid UTF-8, truncated frames or excessive input.
pub fn decode_text(payload: &[u8]) -> Result<Vec<&str>, FrameError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(FrameError::TooLarge);
    }
    let text = core::str::from_utf8(payload).map_err(|_| FrameError::InvalidUtf8)?;
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        if out.len() == MAX_PACKETS {
            return Err(FrameError::TooManyPackets);
        }
        let rest = &text[cursor..];
        let colon = rest.find(':').ok_or(FrameError::InvalidLength)?;
        if colon == 0 {
            return Err(FrameError::EmptyLength);
        }
        let digits = &rest[..colon];
        if !digits.bytes().all(|digit| digit.is_ascii_digit()) {
            return Err(FrameError::InvalidLength);
        }
        let length: usize = digits.parse().map_err(|_| FrameError::InvalidLength)?;
        if length > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge);
        }
        cursor += colon + 1;
        let mut units = 0;
        let mut end = cursor;
        for ch in text[cursor..].chars() {
            if units == length {
                break;
            }
            units += ch.len_utf16();
            if units > length {
                return Err(FrameError::Truncated);
            }
            end += ch.len_utf8();
        }
        if units != length {
            return Err(FrameError::Truncated);
        }
        if end - cursor > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge);
        }
        out.push(&text[cursor..end]);
        cursor = end;
    }
    Ok(out)
}

/// Serialize Engine.IO v3 XHR2 binary payloads carrying *text* packets.
/// Frame header: 0x00, decimal length digits encoded as numeric bytes,
/// 0xff, and the exact UTF-8 payload.
/// # Errors
/// Returns `TooLarge` or `TooManyPackets` when the binary frame bound is exceeded.
pub fn encode_binary(packets: &[&str]) -> Result<Vec<u8>, FrameError> {
    if packets.len() > MAX_PACKETS {
        return Err(FrameError::TooManyPackets);
    }
    let mut bytes = Vec::new();
    for packet in packets {
        if packet.len() > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge);
        }
        let decimal = packet.len().to_string();
        let total = 2 + decimal.len() + packet.len();
        if bytes.len() + total > MAX_PAYLOAD_BYTES {
            return Err(FrameError::TooLarge);
        }
        bytes.push(0);
        bytes.extend(decimal.bytes().map(|c| c - b'0'));
        bytes.push(255);
        bytes.extend(packet.as_bytes());
    }
    Ok(bytes)
}

/// Decode XHR2 binary payload headers without conflating numeric digit bytes
/// with ASCII characters. This adapter deliberately disallows binary Socket.IO
/// attachments: the recovered Vertix event subset requires JSON text frames.
/// # Errors
/// Returns a framing error for invalid types, numeric length headers,
/// invalid UTF-8, truncation or excessive packets.
pub fn decode_binary(payload: &[u8]) -> Result<Vec<&str>, FrameError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(FrameError::TooLarge);
    }
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < payload.len() {
        if out.len() == MAX_PACKETS {
            return Err(FrameError::TooManyPackets);
        }
        if payload[cursor] != 0 {
            return Err(FrameError::UnexpectedFrameType);
        }
        cursor += 1;
        let mut length: usize = 0;
        let mut digit_count = 0;
        while cursor < payload.len() && payload[cursor] != 255 {
            let digit = payload[cursor];
            if digit > 9 || digit_count >= 9 {
                return Err(FrameError::InvalidLength);
            }
            length = length
                .checked_mul(10)
                .and_then(|n| n.checked_add(usize::from(digit)))
                .ok_or(FrameError::TooLarge)?;
            digit_count += 1;
            cursor += 1;
        }
        if digit_count == 0 {
            return Err(FrameError::EmptyLength);
        }
        if cursor == payload.len() {
            return Err(FrameError::Truncated);
        }
        cursor += 1; // 0xff
        if length > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge);
        }
        let end = cursor.checked_add(length).ok_or(FrameError::TooLarge)?;
        let raw = payload.get(cursor..end).ok_or(FrameError::Truncated)?;
        out.push(core::str::from_utf8(raw).map_err(|_| FrameError::InvalidUtf8)?);
        cursor = end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovered_ascii_text_packet() {
        let sample = ["40", "42[\"pong1\"]"];
        let framed = encode_text(&sample).unwrap();
        assert_eq!(framed, b"2:4011:42[\"pong1\"]");
        assert_eq!(decode_text(&framed).unwrap(), sample);
    }

    #[test]
    fn original_xhr2_handshake_digit_header() {
        let packet = format!("0{}", "x".repeat(96));
        let framed = encode_binary(&[&packet]).unwrap();
        assert_eq!(&framed[..4], &[0x00, 0x09, 0x07, 0xff]);
        assert_eq!(decode_binary(&framed).unwrap(), [packet.as_str()]);
    }

    #[test]
    fn multipacket_and_unicode_text() {
        let input = ["40", "42[\"cht\",\"café\"]", "42[\"cht\",\"🗺\"]"];
        assert_eq!(decode_text(&encode_text(&input).unwrap()).unwrap(), input);
        assert_eq!(
            decode_binary(&encode_binary(&input).unwrap()).unwrap(),
            input
        );
    }

    #[test]
    fn separate_utf16_and_utf8_size() {
        assert_eq!(encode_text(&["🗺"]).unwrap(), "2:🗺".as_bytes());
        assert_eq!(encode_binary(&["é"]).unwrap(), [0, 2, 255, 0xc3, 0xa9]);
    }

    #[test]
    fn event_payload_is_not_flattened() {
        assert_eq!(
            inspect_packet("42[\"7\",3,[],[],false]").unwrap(),
            SocketPacket::EventJsonArray("[\"7\",3,[],[],false]")
        );
        assert_eq!(inspect_packet("40").unwrap(), SocketPacket::Connect);
        assert_eq!(inspect_packet("2").unwrap(), SocketPacket::EnginePing);
        assert_eq!(
            inspect_packet("42no-array"),
            Err(FrameError::InvalidSocketPacket)
        );
    }

    #[test]
    fn text_rejects_partial_utf16_and_bad_lengths() {
        for bad in [b"1:".as_slice(), b"4:hi", b"3:ab", b"x:y", b":foo"] {
            assert!(decode_text(bad).is_err(), "{bad:?}");
        }
        assert_eq!(decode_text("1:🗺".as_bytes()), Err(FrameError::Truncated));
    }

    #[test]
    fn binary_rejects_truncation_and_bad_header() {
        for bad in [
            vec![0, 3, 255, b'a'],
            vec![1, 1, 255, b'x'],
            vec![0, 255],
            vec![0, 10, 255, b'x'],
            vec![0, 2],
        ] {
            assert!(decode_binary(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn packet_limits_are_enforced() {
        let packets: Vec<&str> = vec!["40"; MAX_PACKETS + 1];
        assert_eq!(encode_text(&packets), Err(FrameError::TooManyPackets));
        assert_eq!(encode_binary(&packets), Err(FrameError::TooManyPackets));
        assert_eq!(
            decode_text(&vec![b'x'; MAX_PAYLOAD_BYTES + 1]),
            Err(FrameError::TooLarge)
        );
        assert_eq!(
            decode_binary(&vec![0; MAX_PAYLOAD_BYTES + 1]),
            Err(FrameError::TooLarge)
        );
    }
}

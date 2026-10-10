//! What differs between the browser and the desktop build: the WebSocket,
//! HTTP downloads, the launch options and the clock.
//!
//! Both sides offer the same small API, polled once per frame, so the game
//! code never waits on the network.

#![deny(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{Fetch, Socket, launch_options, report, server_base};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{Fetch, Socket, launch_options, report, server_base};

/// Something that happened on a WebSocket since the last poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsEvent {
    Open,
    Message(String),
    Closed(String),
}

/// Wall-clock milliseconds, like JavaScript's `Date.now()`.
#[must_use]
pub fn now_ms() -> f64 {
    macroquad::miniquad::date::now() * 1000.0
}

/// Splits `a=b&c=d` (no leading `?`) into pairs, decoding `%XX` and `+`.
#[must_use]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_pairs_are_decoded() {
        let q = parse_query("room=DEV1&name=Big+Duck%21&flag");
        assert_eq!(
            q,
            vec![
                ("room".into(), "DEV1".into()),
                ("name".into(), "Big Duck!".into()),
                ("flag".into(), String::new()),
            ]
        );
    }
}

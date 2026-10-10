//! A Socket.IO 5 client over Engine.IO 4, WebSocket transport only.
//!
//! KRP's client connects with `io("/<room>")`: one namespace per room. We
//! speak the same packets the reconstruction server's codec does, so the
//! server cannot tell this client from KRP's.

#![forbid(unsafe_code)]

use serde_json::Value;

use crate::platform::{Socket, WsEvent};

/// What the connection reports to the game.
#[derive(Debug, Clone, PartialEq)]
pub enum SioEvent {
    /// The namespace accepted us.
    Connected,
    /// A server `emit`: name, arguments and when it arrived (`now_ms`).
    Event(String, Vec<Value>, f64),
    /// The connection or namespace is gone, with a reason.
    Disconnected(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Opening,
    Joining,
    Joined,
    Closed,
}

pub struct SioClient {
    socket: Socket,
    ns: String,
    state: State,
    queue: Vec<String>,
}

impl SioClient {
    /// Connects to namespace `ns` (e.g. `/DEV0`) on the server at `base`
    /// (`http://host:port`).
    #[must_use]
    pub fn connect(base: &str, ns: &str) -> Self {
        let ws_base = base
            .strip_prefix("https://")
            .map(|r| format!("wss://{r}"))
            .or_else(|| base.strip_prefix("http://").map(|r| format!("ws://{r}")))
            .unwrap_or_else(|| base.to_owned());
        let url = format!("{ws_base}/socket.io/?EIO=4&transport=websocket");
        Self {
            socket: Socket::connect(&url),
            ns: ns.to_owned(),
            state: State::Opening,
            queue: Vec::new(),
        }
    }

    fn prefix(&self) -> String {
        if self.ns == "/" {
            String::new()
        } else {
            format!("{},", self.ns)
        }
    }

    /// Emits `event` with `args`; queued until the namespace is joined.
    pub fn emit(&mut self, event: &str, args: Vec<Value>) {
        if self.state == State::Closed {
            return;
        }
        let mut arr = Vec::with_capacity(args.len() + 1);
        arr.push(Value::String(event.to_owned()));
        arr.extend(args);
        let packet = format!("42{}{}", self.prefix(), Value::Array(arr));
        if self.state == State::Joined {
            self.socket.send(&packet);
        } else {
            self.queue.push(packet);
        }
    }

    pub fn close(&mut self) {
        if self.state != State::Closed {
            if self.state == State::Joined {
                self.socket.send(&format!("41{}", self.prefix()));
            }
            self.socket.close();
            self.state = State::Closed;
        }
    }

    /// Handles everything the socket received since the last poll.
    pub fn poll(&mut self) -> Vec<SioEvent> {
        let mut out = Vec::new();
        for ev in self.socket.poll() {
            match ev {
                WsEvent::Open => {}
                WsEvent::Message(text, at) => self.packet(&text, at, &mut out),
                WsEvent::Closed(reason) => {
                    if self.state != State::Closed {
                        self.state = State::Closed;
                        out.push(SioEvent::Disconnected(reason));
                    }
                }
            }
        }
        out
    }

    fn packet(&mut self, text: &str, at: f64, out: &mut Vec<SioEvent>) {
        let Some(kind) = text.chars().next() else {
            return;
        };
        let body = &text[1..];
        match kind {
            // Engine.IO open: join our namespace.
            '0' => {
                self.state = State::Joining;
                self.socket.send(&format!("40{}", self.prefix()));
            }
            // Engine.IO ping: answer at once. (The browser build answers
            // in vertix_net.js, so a hidden tab stays connected.)
            '2' => self.socket.send("3"),
            '1' => {
                self.state = State::Closed;
                out.push(SioEvent::Disconnected(String::from("server closed")));
            }
            '4' => self.message(body, at, out),
            _ => {}
        }
    }

    fn message(&mut self, body: &str, at: f64, out: &mut Vec<SioEvent>) {
        let Some(kind) = body.chars().next() else {
            return;
        };
        let mut rest = &body[1..];
        // Packets for other namespaces are not ours.
        if rest.starts_with('/') {
            let (ns, tail) = rest.split_once(',').unwrap_or((rest, ""));
            if ns != self.ns {
                return;
            }
            rest = tail;
        } else if self.ns != "/" {
            return;
        }
        // An ack id may precede the payload; KRP's server sends none.
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        match kind {
            '0' => {
                self.state = State::Joined;
                for p in std::mem::take(&mut self.queue) {
                    self.socket.send(&p);
                }
                out.push(SioEvent::Connected);
            }
            '1' => {
                self.state = State::Closed;
                out.push(SioEvent::Disconnected(String::from("namespace closed")));
            }
            '4' => {
                self.state = State::Closed;
                out.push(SioEvent::Disconnected(format!("refused: {rest}")));
            }
            '2' => {
                if let Ok(Value::Array(mut arr)) = serde_json::from_str::<Value>(rest) {
                    if arr.is_empty() {
                        return;
                    }
                    let name = arr.remove(0);
                    if let Value::String(name) = name {
                        out.push(SioEvent::Event(name, arr, at));
                    }
                }
            }
            _ => {}
        }
    }
}

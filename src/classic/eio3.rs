//! Engine.IO 3 server over HTTP long-polling, with Socket.IO 1.x on top:
//! the protocol the archived 2016 client speaks.
//!
//! The transport knows nothing about the game. It turns each client into a
//! [`ClientHandle`] and reports [`TransportEvent`]s on a channel; the game
//! answers through the handle. That keeps the game testable without HTTP.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rand::Rng;
use rand::distr::Alphanumeric;
use serde_json::json;
use tokio::sync::{Notify, mpsc};

use super::codec3 as codec;
use super::sio1::{self as sio, Event, Incoming};
use crate::trace::Trace;
use codec::{Packet, PacketType};

/// Identifies one client connection for the game.
pub type ConnId = u64;

/// Timing the server announces in its open packet. The defaults are the
/// values in the archived 2016 handshakes.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub ping_interval: Duration,
    pub ping_timeout: Duration,
    /// How long a poll is held open with nothing to send before it is
    /// answered with a noop. The 2016 client pings every `ping_interval`,
    /// which also releases a held poll, so this is only a backstop.
    pub poll_hold: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(25),
            ping_timeout: Duration::from_secs(60),
            poll_hold: Duration::from_secs(30),
        }
    }
}

/// Packets queued for one client beyond this mean it stopped polling.
const MAX_OUTBOX: usize = 20_000;

/// What the transport tells the game.
#[derive(Debug)]
pub enum TransportEvent {
    /// `host` is the request's Host header without the port: the name the
    /// client reached this server by, used in the shareable server key.
    Connected {
        conn: ConnId,
        handle: ClientHandle,
        host: String,
    },
    Event {
        conn: ConnId,
        event: Event,
    },
    Disconnected {
        conn: ConnId,
        reason: &'static str,
    },
}

struct SessionState {
    outbox: Vec<Packet>,
    last_seen: Instant,
    closed: bool,
    /// Bumped by every poll so an older, still-held poll can tell it was
    /// replaced and return.
    poll_gen: u64,
}

struct Session {
    sid: String,
    conn: ConnId,
    state: Mutex<SessionState>,
    notify: Notify,
}

impl Session {
    fn lock(&self) -> MutexGuard<'_, SessionState> {
        // A panic while holding the lock leaves plain data behind; keep going.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn push(&self, packet: Packet) -> bool {
        let mut st = self.lock();
        if st.closed {
            return false;
        }
        if st.outbox.len() >= MAX_OUTBOX {
            st.closed = true;
            drop(st);
            self.notify.notify_waiters();
            return false;
        }
        st.outbox.push(packet);
        drop(st);
        // Every waiting poll re-checks; a replaced one returns a noop.
        self.notify.notify_waiters();
        true
    }
}

/// The game's way to talk to one client.
#[derive(Clone)]
pub struct ClientHandle {
    session: Arc<Session>,
    trace: Trace,
}

impl std::fmt::Debug for ClientHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientHandle")
            .field("conn", &self.session.conn)
            .finish_non_exhaustive()
    }
}

impl ClientHandle {
    /// Queues a Socket.IO event for the client.
    pub fn emit(&self, event: &Event) {
        self.trace.event("out", self.session.conn, event);
        self.session.push(Packet::message(sio::encode(event)));
    }

    /// Closes the connection (Socket.IO disconnect, then Engine.IO close).
    pub fn close(&self) {
        self.session.push(Packet::message("1"));
        self.session.push(Packet::new(PacketType::Close, ""));
        self.session.lock().closed = true;
    }

    #[must_use]
    pub fn conn(&self) -> ConnId {
        self.session.conn
    }
}

/// HTTP-level answer for the Engine.IO endpoint.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Reply {
    fn ok_text(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/html",
            body: body.as_bytes().to_vec(),
        }
    }

    fn error(code: u8, message: &str) -> Self {
        Self {
            status: 400,
            content_type: "application/json",
            body: json!({"code": code, "message": message})
                .to_string()
                .into_bytes(),
        }
    }

    fn payload(packets: &[Packet], b64: bool) -> Self {
        if b64 {
            Self {
                status: 200,
                content_type: "text/plain; charset=UTF-8",
                body: codec::encode_payload_string(packets).into_bytes(),
            }
        } else {
            Self {
                status: 200,
                content_type: "application/octet-stream",
                body: codec::encode_payload_binary(packets),
            }
        }
    }
}

/// The query parameters Engine.IO polling requests carry.
#[derive(Debug, Default, serde::Deserialize)]
pub struct Query {
    #[serde(rename = "EIO")]
    pub eio: Option<String>,
    pub transport: Option<String>,
    pub sid: Option<String>,
    pub b64: Option<String>,
    pub j: Option<String>,
}

/// All live Engine.IO sessions.
pub struct Server {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    next_conn: AtomicU64,
    timing: Timing,
    events: mpsc::UnboundedSender<TransportEvent>,
    trace: Trace,
}

impl Server {
    #[must_use]
    pub fn new(
        timing: Timing,
        events: mpsc::UnboundedSender<TransportEvent>,
        trace: Trace,
    ) -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
            timing,
            events,
            trace,
        })
    }

    fn sessions(&self) -> MutexGuard<'_, HashMap<String, Arc<Session>>> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn find(&self, sid: &str) -> Option<Arc<Session>> {
        self.sessions().get(sid).cloned()
    }

    fn check(q: &Query) -> Result<(), Reply> {
        if q.eio.as_deref() != Some("3") {
            return Err(Reply::error(5, "Unsupported protocol version"));
        }
        if q.transport.as_deref() != Some("polling") {
            return Err(Reply::error(0, "Transport unknown"));
        }
        if q.j.is_some() {
            // JSONP polling is only used by browsers without XHR2.
            return Err(Reply::error(0, "Transport unknown"));
        }
        Ok(())
    }

    /// Handles `GET /socket.io/`: a handshake or a long-poll.
    pub async fn get(&self, q: &Query, host: &str) -> Reply {
        if let Err(r) = Self::check(q) {
            return r;
        }
        let b64 = q.b64.is_some();
        let Some(sid) = q.sid.as_deref() else {
            return self.handshake(b64, host);
        };
        let Some(session) = self.find(sid) else {
            return Reply::error(1, "Session ID unknown");
        };
        let my_gen = {
            let mut st = session.lock();
            st.last_seen = Instant::now();
            st.poll_gen += 1;
            st.poll_gen
        };
        // Wake any older poll so it notices it was replaced.
        session.notify.notify_waiters();
        let deadline = tokio::time::Instant::now() + self.timing.poll_hold;
        loop {
            let notified = session.notify.notified();
            {
                let mut st = session.lock();
                if st.poll_gen != my_gen {
                    return Reply::payload(&[Packet::new(PacketType::Noop, "")], b64);
                }
                if !st.outbox.is_empty() {
                    let packets = std::mem::take(&mut st.outbox);
                    let closed = st.closed;
                    drop(st);
                    if closed {
                        self.remove(&session, "server close");
                    }
                    return Reply::payload(&packets, b64);
                }
                if st.closed {
                    drop(st);
                    self.remove(&session, "server close");
                    return Reply::payload(&[Packet::new(PacketType::Close, "")], b64);
                }
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Reply::payload(&[Packet::new(PacketType::Noop, "")], b64);
            }
        }
    }

    fn handshake(&self, b64: bool, host: &str) -> Reply {
        let sid: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(20)
            .map(char::from)
            .collect();
        let conn = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let open = json!({
            "sid": sid,
            // WebSocket upgrade is not implemented yet; see docs/DEVIATIONS.md.
            "upgrades": [],
            "pingInterval": u64::try_from(self.timing.ping_interval.as_millis()).unwrap_or(u64::MAX),
            "pingTimeout": u64::try_from(self.timing.ping_timeout.as_millis()).unwrap_or(u64::MAX),
        });
        let session = Arc::new(Session {
            sid: sid.clone(),
            conn,
            state: Mutex::new(SessionState {
                outbox: Vec::new(),
                last_seen: Instant::now(),
                closed: false,
                poll_gen: 0,
            }),
            notify: Notify::new(),
        });
        self.sessions().insert(sid, session.clone());
        self.trace.note("connect", conn, "engine.io handshake");
        let handle = ClientHandle {
            session,
            trace: self.trace.clone(),
        };
        // The open packet and Socket.IO's connect go out together.
        let packets = [
            Packet::new(PacketType::Open, open.to_string()),
            Packet::message(sio::CONNECT),
        ];
        let _ = self.events.send(TransportEvent::Connected {
            conn,
            handle,
            host: host.to_owned(),
        });
        Reply::payload(&packets, b64)
    }

    /// Handles `POST /socket.io/`: packets from the client.
    pub fn post(&self, q: &Query, body: &[u8]) -> Reply {
        if let Err(r) = Self::check(q) {
            return r;
        }
        let Some(session) = q.sid.as_deref().and_then(|sid| self.find(sid)) else {
            return Reply::error(1, "Session ID unknown");
        };
        session.lock().last_seen = Instant::now();
        let Ok(text) = std::str::from_utf8(body) else {
            return Reply::error(3, "Bad request");
        };
        let packets = match codec::decode_payload_string(text) {
            Ok(p) => p,
            Err(e) => {
                self.trace.note("bad-payload", session.conn, &e.to_string());
                return Reply::error(3, "Bad request");
            }
        };
        for p in packets {
            match p.kind {
                PacketType::Ping => {
                    session.push(Packet::new(PacketType::Pong, p.data));
                }
                PacketType::Close => {
                    self.remove(&session, "client close");
                }
                PacketType::Message => match sio::decode(&p.data) {
                    Incoming::Event(event) => {
                        self.trace.event("in", session.conn, &event);
                        let _ = self.events.send(TransportEvent::Event {
                            conn: session.conn,
                            event,
                        });
                    }
                    Incoming::Disconnect => self.remove(&session, "client disconnect"),
                    Incoming::Connect | Incoming::Unsupported => {}
                },
                _ => {}
            }
        }
        Reply::ok_text("ok")
    }

    fn remove(&self, session: &Arc<Session>, reason: &'static str) {
        let removed = self.sessions().remove(&session.sid).is_some();
        session.lock().closed = true;
        session.notify.notify_waiters();
        if removed {
            self.trace.note("disconnect", session.conn, reason);
            let _ = self.events.send(TransportEvent::Disconnected {
                conn: session.conn,
                reason,
            });
        }
    }

    /// Closes sessions that have not been heard from within
    /// ping interval + ping timeout. Call periodically.
    pub fn reap(&self) {
        let limit = self.timing.ping_interval + self.timing.ping_timeout;
        let stale: Vec<Arc<Session>> = self
            .sessions()
            .values()
            .filter(|s| s.lock().last_seen.elapsed() > limit)
            .cloned()
            .collect();
        for s in stale {
            self.remove(&s, "ping timeout");
        }
    }

    /// Number of open sessions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sessions().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(sid: Option<&str>) -> Query {
        Query {
            eio: Some("3".into()),
            transport: Some("polling".into()),
            sid: sid.map(str::to_owned),
            b64: Some("1".into()),
            j: None,
        }
    }

    fn string_packets(r: &Reply) -> Vec<Packet> {
        codec::decode_payload_string(std::str::from_utf8(&r.body).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn handshake_ping_event_and_close() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let r = server.get(&query(None), "h").await;
        let packets = string_packets(&r);
        assert_eq!(packets[0].kind, PacketType::Open);
        assert_eq!(packets[1], Packet::message("0"));
        let open: serde_json::Value = serde_json::from_str(&packets[0].data).unwrap();
        assert_eq!(open["pingInterval"], 25_000);
        assert_eq!(open["pingTimeout"], 60_000);
        let sid = open["sid"].as_str().unwrap().to_owned();
        assert_eq!(sid.len(), 20);
        let TransportEvent::Connected { conn, handle, .. } = rx.recv().await.unwrap() else {
            panic!("expected connect");
        };

        let body = "1:211:42[\"ping1\"]";
        assert_eq!(server.post(&query(Some(&sid)), body.as_bytes()).status, 200);
        let TransportEvent::Event { event, .. } = rx.recv().await.unwrap() else {
            panic!("expected event");
        };
        assert_eq!(event.name, "ping1");
        handle.emit(&Event::new("pong1", vec![]));
        let got = string_packets(&server.get(&query(Some(&sid)), "h").await);
        assert_eq!(
            got,
            vec![
                Packet::new(PacketType::Pong, ""),
                Packet::message("2[\"pong1\"]")
            ]
        );

        assert_eq!(server.post(&query(Some(&sid)), b"1:1").status, 200);
        let TransportEvent::Disconnected { conn: gone, .. } = rx.recv().await.unwrap() else {
            panic!("expected disconnect");
        };
        assert_eq!(gone, conn);
        assert!(server.is_empty());
        assert_eq!(server.get(&query(Some(&sid)), "h").await.status, 400);
    }

    #[tokio::test]
    async fn binary_handshake_by_default() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let mut q = query(None);
        q.b64 = None;
        let r = server.get(&q, "h").await;
        assert_eq!(r.content_type, "application/octet-stream");
        assert_eq!(r.body[0], 0x00);
        let sep = r.body.iter().position(|&b| b == 0xFF).unwrap();
        assert_eq!(r.body[sep + 1], b'0');
    }

    #[tokio::test(start_paused = true)]
    async fn held_poll_returns_noop_and_reaper_closes() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let timing = Timing {
            ping_interval: Duration::from_millis(10),
            ping_timeout: Duration::from_millis(10),
            poll_hold: Duration::from_millis(50),
        };
        let server = Server::new(timing, tx, Trace::disabled());
        let packets = string_packets(&server.get(&query(None), "h").await);
        let open: serde_json::Value = serde_json::from_str(&packets[0].data).unwrap();
        let sid = open["sid"].as_str().unwrap().to_owned();
        let _ = rx.recv().await;
        let got = string_packets(&server.get(&query(Some(&sid)), "h").await);
        assert_eq!(got, vec![Packet::new(PacketType::Noop, "")]);
        // Instant (std) is not paused; wait out the real limit.
        std::thread::sleep(Duration::from_millis(30));
        server.reap();
        assert!(matches!(
            rx.recv().await,
            Some(TransportEvent::Disconnected {
                reason: "ping timeout",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn rejects_other_protocols() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let mut q = query(None);
        q.eio = Some("4".into());
        assert_eq!(server.get(&q, "h").await.status, 400);
        let mut q = query(None);
        q.transport = Some("websocket".into());
        assert_eq!(server.get(&q, "h").await.status, 400);
    }
}

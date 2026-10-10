//! Engine.IO 4 server (long-polling and WebSocket) with Socket.IO 5
//! namespaces on top.
//!
//! The transport knows nothing about the game. Each Socket.IO namespace a
//! client connects to becomes one [`ClientHandle`] and is reported as a
//! [`TransportEvent`] on a channel; the game accepts or refuses it and
//! answers through the handle. That keeps the game testable without HTTP.
//!
//! KRP's client opens one namespace per room on a single Engine.IO
//! connection, and may leave one room and join another on the same
//! connection, so one Engine.IO session can carry several sockets.

pub mod codec;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rand::Rng;
use rand::distr::Alphanumeric;
use serde_json::json;
use tokio::sync::{Notify, mpsc};

use crate::sio::{self, Event, Incoming};
use crate::trace::Trace;
use codec::{Packet, PacketType};

/// Identifies one Socket.IO socket (one namespace on one connection).
pub type ConnId = u64;

/// Heartbeat and polling timing, announced in the open packet.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// The server pings this often; the client must answer.
    pub ping_interval: Duration,
    /// How long after a ping the client may take to answer.
    pub ping_timeout: Duration,
    /// How long a poll is held open with nothing to send. Server pings
    /// release held polls, so this is only a backstop.
    pub poll_hold: Duration,
}

impl Default for Timing {
    /// The socket.io 4 server defaults, which KRP's server uses.
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(25),
            ping_timeout: Duration::from_secs(20),
            poll_hold: Duration::from_secs(30),
        }
    }
}

/// Largest message the client may send, announced as `maxPayload`.
pub const MAX_PAYLOAD: usize = 64 * 1024;

/// Packets queued for one client beyond this mean it stopped reading.
const MAX_OUTBOX: usize = 20_000;

/// What the transport tells the game.
#[derive(Debug)]
pub enum TransportEvent {
    /// A client asked to join namespace `ns`. The game must call
    /// [`ClientHandle::accept`] or [`ClientHandle::refuse`]. `host` is the
    /// request's Host header without the port.
    Connected {
        conn: ConnId,
        handle: ClientHandle,
        ns: String,
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
    /// Set once the client upgraded to WebSocket: packets go here.
    ws: Option<mpsc::UnboundedSender<String>>,
    /// Open namespace sockets on this connection.
    sockets: HashMap<String, ConnId>,
}

struct Session {
    sid: String,
    host: String,
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
        if let Some(ws) = &st.ws {
            if ws.send(packet.encode()).is_err() {
                st.closed = true;
            }
            return !st.closed;
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

/// The game's way to talk to one socket.
#[derive(Clone)]
pub struct ClientHandle {
    session: Arc<Session>,
    ns: String,
    conn: ConnId,
    trace: Trace,
}

impl std::fmt::Debug for ClientHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientHandle")
            .field("conn", &self.conn)
            .field("ns", &self.ns)
            .finish_non_exhaustive()
    }
}

impl ClientHandle {
    /// Confirms the namespace connect.
    pub fn accept(&self) {
        let sid = format!("{}-{}", self.session.sid, self.conn);
        self.session
            .push(Packet::message(sio::connect_ok(&self.ns, &sid)));
    }

    /// Refuses the namespace connect; no events follow.
    pub fn refuse(&self, message: &str) {
        self.session.lock().sockets.remove(&self.ns);
        self.session
            .push(Packet::message(sio::connect_error(&self.ns, message)));
    }

    /// Queues a Socket.IO event for the client.
    pub fn emit(&self, event: &Event) {
        self.trace.event("out", self.conn, event);
        self.session.push(Packet::message(event.encode(&self.ns)));
    }

    /// Closes this socket (the Engine.IO connection stays up).
    pub fn close(&self) {
        self.session.lock().sockets.remove(&self.ns);
        self.session
            .push(Packet::message(sio::disconnect(&self.ns)));
    }

    #[must_use]
    pub fn conn(&self) -> ConnId {
        self.conn
    }

    #[must_use]
    pub fn ns(&self) -> &str {
        &self.ns
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

    fn payload(packets: &[Packet]) -> Self {
        Self {
            status: 200,
            content_type: "text/plain; charset=UTF-8",
            body: codec::encode_payload(packets).into_bytes(),
        }
    }
}

/// The query parameters Engine.IO requests carry.
#[derive(Debug, Default, serde::Deserialize)]
pub struct Query {
    #[serde(rename = "EIO")]
    pub eio: Option<String>,
    pub transport: Option<String>,
    pub sid: Option<String>,
}

/// All live Engine.IO sessions.
pub struct Server {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    next_conn: AtomicU64,
    timing: Timing,
    events: mpsc::UnboundedSender<TransportEvent>,
    trace: Trace,
}

/// A WebSocket attached to a session, for the HTTP layer to pump.
pub struct WsLink {
    session: Arc<Session>,
    tx: mpsc::UnboundedSender<String>,
    /// Text frames to send to the client.
    pub outgoing: mpsc::UnboundedReceiver<String>,
}

impl WsLink {
    /// Resolves once the session behind this link has ended, so the HTTP
    /// layer can close the socket.
    pub fn ended(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let session = self.session.clone();
        async move {
            loop {
                let notified = session.notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if session.lock().closed {
                    return;
                }
                notified.await;
            }
        }
    }
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

    fn check(q: &Query, transport: &str) -> Result<(), Reply> {
        if q.eio.as_deref() != Some("4") {
            return Err(Reply::error(5, "Unsupported protocol version"));
        }
        if q.transport.as_deref() != Some(transport) {
            return Err(Reply::error(0, "Transport unknown"));
        }
        Ok(())
    }

    fn open_packet(&self, sid: &str) -> Packet {
        let ms = |d: Duration| u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        let open = json!({
            "sid": sid,
            "upgrades": ["websocket"],
            "pingInterval": ms(self.timing.ping_interval),
            "pingTimeout": ms(self.timing.ping_timeout),
            "maxPayload": MAX_PAYLOAD,
        });
        Packet::new(PacketType::Open, open.to_string())
    }

    fn new_session(&self, host: &str, ws: Option<mpsc::UnboundedSender<String>>) -> Arc<Session> {
        let sid: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(20)
            .map(char::from)
            .collect();
        let session = Arc::new(Session {
            sid: sid.clone(),
            host: host.to_owned(),
            state: Mutex::new(SessionState {
                outbox: Vec::new(),
                last_seen: Instant::now(),
                closed: false,
                poll_gen: 0,
                ws,
                sockets: HashMap::new(),
            }),
            notify: Notify::new(),
        });
        self.sessions().insert(sid, session.clone());
        self.trace.note("connect", 0, "engine.io handshake");
        session
    }

    /// Handles `GET /socket.io/?transport=polling`: a handshake or a poll.
    pub async fn get(&self, q: &Query, host: &str) -> Reply {
        if let Err(r) = Self::check(q, "polling") {
            return r;
        }
        let Some(sid) = q.sid.as_deref() else {
            let session = self.new_session(host, None);
            return Reply::payload(&[self.open_packet(&session.sid)]);
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
                if st.poll_gen != my_gen || st.ws.is_some() {
                    return Reply::payload(&[Packet::new(PacketType::Noop, "")]);
                }
                if !st.outbox.is_empty() {
                    let packets = std::mem::take(&mut st.outbox);
                    return Reply::payload(&packets);
                }
                if st.closed {
                    drop(st);
                    self.remove(&session, "server close");
                    return Reply::payload(&[Packet::new(PacketType::Close, "")]);
                }
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Reply::payload(&[Packet::new(PacketType::Noop, "")]);
            }
        }
    }

    /// Handles `POST /socket.io/`: packets from the client.
    pub fn post(&self, q: &Query, body: &[u8]) -> Reply {
        if let Err(r) = Self::check(q, "polling") {
            return r;
        }
        let Some(session) = q.sid.as_deref().and_then(|sid| self.find(sid)) else {
            return Reply::error(1, "Session ID unknown");
        };
        let Ok(text) = std::str::from_utf8(body) else {
            return Reply::error(3, "Bad request");
        };
        let packets = match codec::decode_payload(text) {
            Ok(p) => p,
            Err(e) => {
                self.trace.note("bad-payload", 0, &e.to_string());
                return Reply::error(3, "Bad request");
            }
        };
        for p in packets {
            self.incoming(&session, &p);
        }
        Reply::ok_text("ok")
    }

    /// Attaches a WebSocket: an upgrade of the polling session `sid`, or a
    /// new session when `sid` is `None` (WebSocket-only clients).
    ///
    /// # Errors
    /// Returns the HTTP error reply when the request is not acceptable.
    pub fn ws_open(&self, q: &Query, host: &str) -> Result<WsLink, Reply> {
        Self::check(q, "websocket")?;
        let (tx, outgoing) = mpsc::unbounded_channel();
        let session = if let Some(sid) = q.sid.as_deref() {
            self.find(sid)
                .ok_or_else(|| Reply::error(1, "Session ID unknown"))?
        } else {
            let session = self.new_session(host, Some(tx.clone()));
            let _ = tx.send(self.open_packet(&session.sid).encode());
            session
        };
        Ok(WsLink {
            session,
            tx,
            outgoing,
        })
    }

    /// Handles one text frame from a WebSocket.
    pub fn ws_message(&self, link: &WsLink, text: &str) {
        match text {
            // Upgrade probe: answered on the new socket only.
            "2probe" => {
                let _ = link.tx.send("3probe".into());
            }
            // Upgrade done: everything goes over the WebSocket from now on.
            "5" => {
                let mut st = link.session.lock();
                for p in std::mem::take(&mut st.outbox) {
                    let _ = link.tx.send(p.encode());
                }
                st.ws = Some(link.tx.clone());
                st.last_seen = Instant::now();
                drop(st);
                link.session.notify.notify_waiters();
            }
            _ => match Packet::decode(text) {
                Ok(p) => self.incoming(&link.session, &p),
                Err(e) => self.trace.note("bad-frame", 0, &e.to_string()),
            },
        }
    }

    /// The WebSocket closed. If it carried the session, the session ends.
    pub fn ws_closed(&self, link: &WsLink) {
        let carried = link
            .session
            .lock()
            .ws
            .as_ref()
            .is_some_and(|w| w.same_channel(&link.tx));
        if carried {
            self.remove(&link.session, "transport close");
        }
    }

    fn incoming(&self, session: &Arc<Session>, p: &Packet) {
        session.lock().last_seen = Instant::now();
        match p.kind {
            PacketType::Close => self.remove(session, "client close"),
            PacketType::Message => self.message(session, &p.data),
            // Pong answers our ping; last_seen is already updated.
            _ => {}
        }
    }

    fn message(&self, session: &Arc<Session>, data: &str) {
        match sio::decode(data) {
            Incoming::Connect { ns } => {
                let conn = self.next_conn.fetch_add(1, Ordering::Relaxed);
                let old = session.lock().sockets.insert(ns.clone(), conn);
                if let Some(old) = old {
                    self.disconnected(old, "reconnect");
                }
                self.trace.note("connect", conn, &ns);
                let handle = ClientHandle {
                    session: session.clone(),
                    ns: ns.clone(),
                    conn,
                    trace: self.trace.clone(),
                };
                let _ = self.events.send(TransportEvent::Connected {
                    conn,
                    handle,
                    ns,
                    host: session.host.clone(),
                });
            }
            Incoming::Disconnect { ns } => {
                let conn = session.lock().sockets.remove(&ns);
                if let Some(conn) = conn {
                    self.disconnected(conn, "client disconnect");
                }
            }
            Incoming::Event { ns, event } => {
                let conn = session.lock().sockets.get(&ns).copied();
                if let Some(conn) = conn {
                    self.trace.event("in", conn, &event);
                    let _ = self.events.send(TransportEvent::Event { conn, event });
                }
            }
            Incoming::Unsupported => {}
        }
    }

    fn disconnected(&self, conn: ConnId, reason: &'static str) {
        self.trace.note("disconnect", conn, reason);
        let _ = self
            .events
            .send(TransportEvent::Disconnected { conn, reason });
    }

    fn remove(&self, session: &Arc<Session>, reason: &'static str) {
        let removed = self.sessions().remove(&session.sid).is_some();
        let sockets: Vec<ConnId> = {
            let mut st = session.lock();
            st.closed = true;
            st.ws = None;
            st.sockets.drain().map(|(_, c)| c).collect()
        };
        session.notify.notify_waiters();
        if removed {
            for conn in sockets {
                self.disconnected(conn, reason);
            }
        }
    }

    /// Pings every client and closes those that have not been heard from
    /// within ping interval + ping timeout. Call every ping interval.
    pub fn heartbeat(&self) {
        let limit = self.timing.ping_interval + self.timing.ping_timeout;
        let all: Vec<Arc<Session>> = self.sessions().values().cloned().collect();
        for s in all {
            if s.lock().last_seen.elapsed() > limit {
                self.remove(&s, "ping timeout");
            } else {
                s.push(Packet::new(PacketType::Ping, ""));
            }
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
            eio: Some("4".into()),
            transport: Some("polling".into()),
            sid: sid.map(str::to_owned),
        }
    }

    fn packets(r: &Reply) -> Vec<Packet> {
        codec::decode_payload(std::str::from_utf8(&r.body).unwrap()).unwrap()
    }

    async fn open(server: &Server) -> String {
        let p = packets(&server.get(&query(None), "h").await);
        assert_eq!(p[0].kind, PacketType::Open);
        let open: serde_json::Value = serde_json::from_str(&p[0].data).unwrap();
        assert_eq!(open["upgrades"], json!(["websocket"]));
        open["sid"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn namespace_connect_event_and_disconnect() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let sid = open(&server).await;

        server.post(&query(Some(&sid)), b"40/DEV0,");
        let TransportEvent::Connected {
            conn, handle, ns, ..
        } = rx.recv().await.unwrap()
        else {
            panic!("expected connect");
        };
        assert_eq!(ns, "/DEV0");
        handle.accept();
        let got = packets(&server.get(&query(Some(&sid)), "h").await);
        assert!(got[0].data.starts_with("0/DEV0,{\"sid\":"));

        server.post(
            &query(Some(&sid)),
            b"42/DEV0,[\"ping1\"]\x1e42/other,[\"x\"]",
        );
        let TransportEvent::Event { event, conn: c } = rx.recv().await.unwrap() else {
            panic!("expected event");
        };
        assert_eq!((event.name.as_str(), c), ("ping1", conn));
        // Events for a namespace the client never joined are dropped.
        assert!(rx.try_recv().is_err());
        handle.emit(&Event::new("pong1", vec![]));
        assert_eq!(
            packets(&server.get(&query(Some(&sid)), "h").await),
            vec![Packet::message("2/DEV0,[\"pong1\"]")]
        );

        server.post(&query(Some(&sid)), b"41/DEV0,");
        assert!(matches!(
            rx.recv().await,
            Some(TransportEvent::Disconnected { conn: c, .. }) if c == conn
        ));
        assert_eq!(server.len(), 1, "the connection outlives the socket");
        server.post(&query(Some(&sid)), b"1");
        assert!(server.is_empty());
    }

    #[tokio::test]
    async fn refused_namespace_gets_a_connect_error() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let sid = open(&server).await;
        server.post(&query(Some(&sid)), b"40/nope,");
        let Some(TransportEvent::Connected { handle, .. }) = rx.recv().await else {
            panic!("expected connect");
        };
        handle.refuse("Invalid namespace");
        let got = packets(&server.get(&query(Some(&sid)), "h").await);
        assert_eq!(got[0].data, r#"4/nope,{"message":"Invalid namespace"}"#);
        server.post(&query(Some(&sid)), b"42/nope,[\"x\"]");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn websocket_upgrade_moves_the_session() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let sid = open(&server).await;
        let mut wsq = query(Some(&sid));
        wsq.transport = Some("websocket".into());
        let mut link = server.ws_open(&wsq, "h").unwrap();
        server.ws_message(&link, "2probe");
        assert_eq!(link.outgoing.recv().await.unwrap(), "3probe");
        server.post(&query(Some(&sid)), b"40/DEV0,");
        let Some(TransportEvent::Connected { handle, .. }) = rx.recv().await else {
            panic!("expected connect");
        };
        handle.accept();
        server.ws_message(&link, "5");
        // Queued before the upgrade, delivered over the socket.
        assert!(link.outgoing.recv().await.unwrap().starts_with("40/DEV0,"));
        // A poll after the upgrade is released with a noop.
        assert_eq!(
            packets(&server.get(&query(Some(&sid)), "h").await),
            vec![Packet::new(PacketType::Noop, "")]
        );
        server.ws_message(&link, "42/DEV0,[\"cht\",\"hi\"]");
        assert!(matches!(
            rx.recv().await,
            Some(TransportEvent::Event { .. })
        ));
        server.ws_closed(&link);
        assert!(matches!(
            rx.recv().await,
            Some(TransportEvent::Disconnected { .. })
        ));
        assert!(server.is_empty());
    }

    #[tokio::test]
    async fn websocket_only_clients_get_an_open_packet() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let server = Server::new(Timing::default(), tx, Trace::disabled());
        let mut q = query(None);
        q.transport = Some("websocket".into());
        let mut link = server.ws_open(&q, "h").unwrap();
        assert!(link.outgoing.recv().await.unwrap().starts_with("0{"));
    }

    #[tokio::test]
    async fn heartbeat_pings_then_closes_silent_clients() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let timing = Timing {
            ping_interval: Duration::from_millis(10),
            ping_timeout: Duration::from_millis(10),
            poll_hold: Duration::from_millis(50),
        };
        let server = Server::new(timing, tx, Trace::disabled());
        let sid = open(&server).await;
        server.post(&query(Some(&sid)), b"40/DEV0,");
        let _ = rx.recv().await;
        server.heartbeat();
        assert_eq!(
            packets(&server.get(&query(Some(&sid)), "h").await),
            vec![Packet::new(PacketType::Ping, "")]
        );
        std::thread::sleep(Duration::from_millis(30));
        server.heartbeat();
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
        q.eio = Some("3".into());
        assert_eq!(server.get(&q, "h").await.status, 400);
    }
}

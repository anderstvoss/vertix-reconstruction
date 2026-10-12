//! The admin port: the panel page, its WebSocket and `POST /api/cmd`.
//!
//! The page itself holds nothing secret. Everything that reads or changes
//! the game needs the admin token: `Authorization: Bearer <token>` on the
//! HTTP API, `?token=` on the WebSocket (browsers cannot set its headers).
//! A browser on another site therefore cannot drive the panel, even though
//! it can reach the port. The panel takes the token from the address's
//! `#token=` part, which the browser never sends to any server.

use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

use super::{Log, Reply, Request, same_token};
use crate::game::Ask;

const PANEL: &str = include_str!("panel.html");
/// Longest command line accepted.
const MAX_LINE: usize = 64 * 1024;

#[derive(Clone)]
pub struct AdminState {
    pub game: mpsc::UnboundedSender<Ask>,
    pub log: Log,
    pub token: Arc<str>,
}

pub fn router(state: AdminState) -> Router {
    Router::new()
        .route("/", get(panel))
        .route("/api/cmd", post(command))
        .route("/ws", get(ws))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_LINE))
        .with_state(state)
}

async fn panel() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
                 connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'",
            ),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        PANEL,
    )
        .into_response()
}

/// Sends one command to the game and waits for its answer.
async fn run(game: &mpsc::UnboundedSender<Ask>, line: String, room: Option<String>) -> Reply {
    let (tx, rx) = oneshot::channel();
    if game
        .send(Ask::Admin(Request {
            line,
            room,
            reply: tx,
        }))
        .is_err()
    {
        return Reply::err("the game has stopped");
    }
    rx.await
        .unwrap_or_else(|_| Reply::err("the game has stopped"))
}

#[derive(Deserialize)]
struct CommandBody {
    line: String,
    #[serde(default)]
    room: Option<String>,
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// `POST /api/cmd` with `{"line": "...", "room": "..."}`: for scripts.
async fn command(
    State(s): State<AdminState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !bearer(&headers).is_some_and(|t| same_token(t, &s.token)) {
        return (StatusCode::UNAUTHORIZED, "admin token needed").into_response();
    }
    let Ok(b) = serde_json::from_slice::<CommandBody>(&body) else {
        return (StatusCode::BAD_REQUEST, "send {\"line\": \"...\"}").into_response();
    };
    let reply = run(&s.game, b.line, b.room).await;
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&reply).unwrap_or_default(),
    )
        .into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    #[serde(default)]
    token: String,
}

/// The origin must be this server's own, if the browser names one.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) else {
        return true;
    };
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    origin
        .split_once("://")
        .is_some_and(|(_, rest)| !host.is_empty() && rest == host)
}

async fn ws(
    State(s): State<AdminState>,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !same_token(&q.token, &s.token) {
        return (StatusCode::UNAUTHORIZED, "admin token needed").into_response();
    }
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "wrong origin").into_response();
    }
    upgrade
        .max_message_size(MAX_LINE)
        .on_upgrade(move |socket| session(s, socket))
}

#[derive(Deserialize)]
struct WsCommand {
    #[serde(default)]
    id: serde_json::Value,
    line: String,
    #[serde(default)]
    room: Option<String>,
}

/// One panel connection: commands in, answers and the live log out.
async fn session(s: AdminState, mut socket: WebSocket) {
    let mut log = s.log.subscribe();
    loop {
        tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { return };
                let Message::Text(text) = msg else { continue };
                let out = match serde_json::from_str::<WsCommand>(&text) {
                    Ok(c) => {
                        let reply = run(&s.game, c.line, c.room).await;
                        json!({"type": "reply", "id": c.id, "reply": reply})
                    }
                    Err(e) => json!({"type": "reply", "id": null,
                        "reply": Reply::err(format!("bad message: {e}"))}),
                };
                if socket.send(Message::Text(out.to_string().into())).await.is_err() {
                    return;
                }
            }
            line = log.recv() => {
                let out = match line {
                    Ok(l) => json!({"type": "log", "line": l}),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        json!({"type": "log", "line": {"t": 0, "room": "", "kind": "admin",
                            "text": format!("({n} log lines skipped)")}})
                    }
                    Err(_) => return,
                };
                if socket.send(Message::Text(out.to_string().into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn origin_must_match_host() {
        let mut h = HeaderMap::new();
        assert!(same_origin(&h));
        h.insert(header::HOST, HeaderValue::from_static("example.test:8082"));
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://example.test:8082"),
        );
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("http://evil.test"));
        assert!(!same_origin(&h));
    }

    #[test]
    fn bearer_token_is_read() {
        let mut h = HeaderMap::new();
        assert_eq!(bearer(&h), None);
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(bearer(&h), Some("abc"));
    }
}

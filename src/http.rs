//! HTTP routes: the client build, KRP's `/api` routes and Socket.IO.
//!
//! The client is `KrunkerRevival`'s (KRP) browser client, built from a KRP
//! checkout into a local directory (`scripts/build-client.sh`). Nothing of
//! it is part of this repository; we serve whatever that directory holds.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::{
    Message, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection,
};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::eio;
use crate::game::Ask;
use crate::trace::Trace;

/// Polling POST bodies; the client's messages are small.
const MAX_POST: usize = eio::MAX_PAYLOAD;

#[derive(Clone)]
pub struct AppState {
    /// The built client (`index.html` and its assets).
    pub client_dir: Arc<PathBuf>,
    pub eio: Arc<eio::Server>,
    pub game: mpsc::UnboundedSender<Ask>,
    pub trace: Trace,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/getIP", get(get_ip))
        .route("/api/getRooms", get(get_rooms))
        .route("/api/getLbs", get(get_lbs))
        .route(
            "/socket.io/",
            get(eio_get).post(eio_post).options(eio_options),
        )
        .fallback(client_file)
        .layer(DefaultBodyLimit::max(MAX_POST))
        .with_state(state)
}

/// Splits a Host header into host and port, keeping IPv6 brackets.
#[must_use]
pub fn split_host(host: &str) -> (String, Option<u16>) {
    if let Some(rest) = host.strip_prefix('[') {
        if let Some((h, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok());
            return (format!("[{h}]"), port);
        }
        return (host.to_owned(), None);
    }
    match host.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h.to_owned(), p.parse().ok()),
        _ => (host.to_owned(), None),
    }
}

fn host_of(headers: &HeaderMap) -> (String, Option<u16>) {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map_or_else(|| (String::new(), None), split_host)
}

fn json_response(body: &Value) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body.to_string(),
    )
        .into_response()
}

async fn ask<T>(s: &AppState, make: impl FnOnce(oneshot::Sender<T>) -> Ask) -> Option<T> {
    let (tx, rx) = oneshot::channel();
    s.game.send(make(tx)).ok()?;
    rx.await.ok()
}

#[derive(serde::Deserialize)]
struct RoomQuery {
    #[serde(default)]
    room: String,
}

/// Which room to join and where. KRP's client ignores `ip` and `port` and
/// connects to the page's own origin, so we answer with the address the
/// request reached us by. Archived replies naming the original servers are
/// never served.
async fn get_ip(
    State(s): State<AppState>,
    Query(q): Query<RoomQuery>,
    headers: HeaderMap,
) -> Response {
    let room = ask(&s, |tx| Ask::Resolve(q.room, tx)).await.flatten();
    let Some(room) = room else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (host, port) = host_of(&headers);
    json_response(&json!({
        "ip": host,
        "region": "local",
        "port": port.unwrap_or(80).to_string(),
        "room": room,
    }))
}

async fn get_rooms(State(s): State<AppState>) -> Response {
    let rooms = ask(&s, Ask::Rooms).await.unwrap_or_else(|| json!([]));
    json_response(&rooms)
}

/// Leaderboards need accounts, which this reconstruction does not have:
/// every board is empty.
async fn get_lbs() -> Response {
    json_response(&json!({
        "rank": [], "kdrThousand": [], "kdrAny": [], "kills": [],
        "clanRank": [], "clanKdr": [],
    }))
}

fn cors(headers: &HeaderMap, res: &mut Response) {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let h = res.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        h.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
    }
}

fn eio_response(reply: eio::Reply, headers: &HeaderMap) -> Response {
    let mut res = Response::new(Body::from(reply.body));
    *res.status_mut() = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_REQUEST);
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(reply.content_type),
    );
    cors(headers, &mut res);
    res
}

async fn eio_get(
    State(s): State<AppState>,
    Query(q): Query<eio::Query>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let (host, _) = host_of(&headers);
    if q.transport.as_deref() == Some("websocket") {
        let Ok(upgrade) = upgrade else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        return match s.eio.ws_open(&q, &host) {
            Ok(link) => upgrade
                .max_message_size(eio::MAX_PAYLOAD)
                .on_upgrade(move |socket| pump(s.eio, link, socket)),
            Err(reply) => eio_response(reply, &headers),
        };
    }
    let reply = s.eio.get(&q, &host).await;
    eio_response(reply, &headers)
}

/// Moves frames between one WebSocket and its Engine.IO session.
async fn pump(server: Arc<eio::Server>, mut link: eio::WsLink, mut socket: WebSocket) {
    let ended = link.ended();
    tokio::pin!(ended);
    loop {
        tokio::select! {
            out = link.outgoing.recv() => {
                let Some(text) = out else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            frame = socket.recv() => match frame {
                Some(Ok(Message::Text(text))) => server.ws_message(&link, text.as_str()),
                Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            },
            () = &mut ended => break,
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    server.ws_closed(&link);
}

async fn eio_post(
    State(s): State<AppState>,
    Query(q): Query<eio::Query>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let reply = s.eio.post(&q, &body);
    eio_response(reply, &headers)
}

async fn eio_options(headers: HeaderMap) -> Response {
    let mut res = StatusCode::OK.into_response();
    cors(&headers, &mut res);
    let h = res.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST"),
    );
    res
}

/// The file under `root` a URL path names, or `None` if the path tries to
/// leave `root`. Directories map to their `index.html`, and an extensionless
/// path to `<path>.html` (the client's other pages).
#[must_use]
pub fn resolve_client_path(root: &Path, url_path: &str) -> Option<Vec<PathBuf>> {
    let rel = Path::new(url_path.trim_start_matches('/'));
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let base = root.join(rel);
    let mut tries = vec![base.join("index.html")];
    if !url_path.ends_with('/') && !rel.as_os_str().is_empty() {
        tries.insert(0, base.clone());
        if rel.extension().is_none() {
            tries.push(base.with_extension("html"));
        }
    }
    Some(tries)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "zip" => "application/zip",
        // The Rust client's browser build (crates/client).
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

async fn client_file(State(s): State<AppState>, method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(tries) = resolve_client_path(&s.client_dir, uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    for path in tries {
        if let Ok(bytes) = tokio::fs::read(&path).await {
            let mut res = Response::new(Body::from(bytes));
            let h = res.headers_mut();
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type(&path)),
            );
            if content_type(&path).starts_with("text/html") {
                h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            }
            return res;
        }
    }
    s.trace.note("not-found", 0, uri.path());
    StatusCode::NOT_FOUND.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_hosts() {
        assert_eq!(
            split_host("example.org:8080"),
            ("example.org".into(), Some(8080))
        );
        assert_eq!(split_host("example.org"), ("example.org".into(), None));
        assert_eq!(
            split_host("[2001:db8::2]:81"),
            ("[2001:db8::2]".into(), Some(81))
        );
        assert_eq!(split_host("2001:db8::2"), ("2001:db8::2".into(), None));
    }

    #[test]
    fn client_paths_stay_inside_the_build() {
        let root = Path::new("build");
        assert_eq!(
            resolve_client_path(root, "/").unwrap(),
            [root.join("index.html")]
        );
        assert_eq!(
            resolve_client_path(root, "/leaderboards").unwrap(),
            [
                root.join("leaderboards"),
                root.join("leaderboards/index.html"),
                root.join("leaderboards.html")
            ]
        );
        assert_eq!(
            resolve_client_path(root, "/res.zip").unwrap()[0],
            root.join("res.zip")
        );
        assert!(resolve_client_path(root, "/../secret").is_none());
        assert!(resolve_client_path(root, "/a/../../b").is_none());
    }
}

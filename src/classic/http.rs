//! HTTP routes for the 2016 client: its page and files from the archive,
//! `/getIP` and Engine.IO 3.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;

use super::eio3 as eio;
use crate::http::split_host;
use crate::originals::{Asset, Store, page};
use crate::trace::Trace;

/// The client's POST bodies are a few hundred bytes; chat is capped at 50
/// characters.
const MAX_POST: usize = 64 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub eio: Arc<eio::Server>,
    pub trace: Trace,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/getIP", get(get_ip))
        .route(
            "/socket.io/",
            get(eio_get).post(eio_post).options(eio_options),
        )
        .fallback(original)
        .layer(DefaultBodyLimit::max(MAX_POST))
        .with_state(state)
}

fn host_of(headers: &HeaderMap) -> (String, Option<u16>) {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map_or_else(|| (String::new(), None), split_host)
}

fn asset_response(asset: &Asset) -> Response {
    let mut res = Response::new(Body::from(Bytes::copy_from_slice(&asset.bytes)));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(asset.content_type),
    );
    res
}

async fn index(State(s): State<AppState>) -> Response {
    let mut res = asset_response(s.store.page());
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(page::CSP),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// The client asks where the game server is and connects to
/// `http://<ip>:<port>`. We answer with the address it reached us by, so
/// the socket is same-origin. Archived `/getIP` replies name the original
/// servers and are never served.
async fn get_ip(headers: HeaderMap) -> Response {
    let (host, port) = host_of(&headers);
    let body = json!({"ip": host, "port": port.unwrap_or(80), "region": "local"});
    (
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
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
) -> Response {
    let (host, _) = host_of(&headers);
    let reply = s.eio.get(&q, &host).await;
    eio_response(reply, &headers)
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

async fn original(State(s): State<AppState>, method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if let Some(asset) = s.store.get(uri.path()) {
        return asset_response(&asset);
    }
    s.trace.note("not-found", 0, uri.path());
    StatusCode::NOT_FOUND.into_response()
}

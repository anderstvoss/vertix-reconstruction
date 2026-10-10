//! Browser build: the page's own WebSocket and `fetch`, reached through
//! `web/vertix_net.js` (a miniquad plugin), polled once per frame.

#![allow(unsafe_code)]

use sapp_jsutils::JsObject;

use super::{WsEvent, parse_query};

// Implemented in web/vertix_net.js. Every function takes plain numbers or
// JsObject handles (indexes into a JavaScript table), never pointers.
unsafe extern "C" {
    fn vx_ws_open(url: JsObject) -> i32;
    fn vx_ws_send(id: i32, text: JsObject);
    fn vx_ws_next(id: i32) -> JsObject;
    fn vx_ws_close(id: i32);
    fn vx_fetch(url: JsObject) -> i32;
    fn vx_fetch_poll(id: i32) -> JsObject;
    fn vx_page_url() -> JsObject;
    fn vx_report(json: JsObject);
}

/// Publishes measurements as `window.vertixMetrics` for the test browser.
pub fn report(json: &str, _options: &[(String, String)]) {
    // SAFETY: the plugin takes ownership of the string handle.
    unsafe { vx_report(JsObject::string(json)) };
}

/// Tells miniquad's loader that this build uses the `vertix_net` plugin
/// (it warns about bundled plugins the wasm does not claim).
#[unsafe(no_mangle)]
pub extern "C" fn vertix_net_crate_version() -> u32 {
    1
}

fn page_url() -> String {
    let mut s = String::new();
    // SAFETY: takes no arguments and returns a handle the plugin created.
    unsafe { vx_page_url() }.to_string(&mut s);
    s
}

/// Launch options from the page's query string (`?room=DEV1&name=...`).
#[must_use]
pub fn launch_options() -> Vec<(String, String)> {
    let url = page_url();
    let query = url.split_once('?').map_or("", |(_, q)| q);
    let query = query.split('#').next().unwrap_or("");
    parse_query(query)
}

/// The server that served this page, unless `?server=` names another.
#[must_use]
pub fn server_base(options: &[(String, String)]) -> String {
    if let Some((_, v)) = options.iter().find(|(k, _)| k == "server") {
        return v.trim_end_matches('/').to_owned();
    }
    let url = page_url();
    let Some((scheme, rest)) = url.split_once("://") else {
        return String::new();
    };
    let host = rest.split('/').next().unwrap_or("");
    format!("{scheme}://{host}")
}

/// A WebSocket owned by the page.
pub struct Socket {
    id: i32,
}

impl Socket {
    #[must_use]
    pub fn connect(url: &str) -> Self {
        // SAFETY: the plugin takes ownership of the string handle.
        let id = unsafe { vx_ws_open(JsObject::string(url)) };
        Self { id }
    }

    pub fn send(&self, text: &str) {
        // SAFETY: `id` came from vx_ws_open; the plugin takes the handle.
        unsafe { vx_ws_send(self.id, JsObject::string(text)) };
    }

    pub fn close(&self) {
        // SAFETY: closing an unknown or closed id is a no-op in the plugin.
        unsafe { vx_ws_close(self.id) };
    }

    pub fn poll(&mut self) -> Vec<WsEvent> {
        let mut out = Vec::new();
        loop {
            // SAFETY: returns a fresh handle (or nil) that we own and drop.
            let ev = unsafe { vx_ws_next(self.id) };
            if ev.is_nil() || ev.is_undefined() {
                break;
            }
            let mut kind = String::new();
            ev.field("t").to_string(&mut kind);
            let mut data = String::new();
            ev.field("d").to_string(&mut data);
            out.push(match kind.as_str() {
                "open" => WsEvent::Open,
                "msg" => WsEvent::Message(data),
                _ => WsEvent::Closed(data),
            });
        }
        out
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.close();
    }
}

/// A `fetch` running in the page.
pub struct Fetch {
    id: i32,
    done: bool,
}

impl Fetch {
    #[must_use]
    pub fn start(url: &str) -> Self {
        // SAFETY: the plugin takes ownership of the string handle.
        let id = unsafe { vx_fetch(JsObject::string(url)) };
        Self { id, done: false }
    }

    pub fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        if self.done {
            return None;
        }
        // SAFETY: returns a fresh handle (or nil) that we own and drop.
        let res = unsafe { vx_fetch_poll(self.id) };
        if res.is_nil() || res.is_undefined() {
            return None;
        }
        self.done = true;
        if res.field_u32("ok") == 1 {
            let mut data = Vec::new();
            res.field("d").to_byte_buffer(&mut data);
            Some(Ok(data))
        } else {
            let mut err = String::new();
            res.field("d").to_string(&mut err);
            Some(Err(err))
        }
    }
}

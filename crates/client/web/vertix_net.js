// WebSocket and fetch for the Rust client's browser build, as a miniquad
// plugin. Rust polls these once per frame (src/platform/web.rs); nothing
// here calls back into Rust.
//
// Browsers stop drawing frames in a hidden tab, so messages wait here until
// the tab is shown again. Each one carries the time it arrived, and the
// Engine.IO ping is answered here, as it arrives, so the server keeps a
// hidden tab connected.
"use strict";

(function () {
    var sockets = {};
    var nextSocket = 1;
    var fetches = {};
    var nextFetch = 1;

    function register_plugin(importObject) {
        importObject.env.vx_ws_open = function (url) {
            var id = nextSocket++;
            var queue = [];
            var ws = null;
            try {
                ws = new WebSocket(consume_js_object(url));
                ws.onopen = function () { queue.push({ t: "open", d: "" }); };
                ws.onmessage = function (ev) {
                    if (typeof ev.data !== "string") return;
                    if (ev.data === "2") {
                        ws.send("3");
                        return;
                    }
                    queue.push({ t: "msg", d: ev.data, at: String(Date.now()) });
                };
                ws.onclose = function (ev) { queue.push({ t: "close", d: "closed (" + ev.code + ")" }); };
                ws.onerror = function () { queue.push({ t: "close", d: "connection error" }); };
            } catch (e) {
                queue.push({ t: "close", d: String(e) });
            }
            sockets[id] = { ws: ws, queue: queue };
            return id;
        };
        importObject.env.vx_ws_send = function (id, text) {
            var s = sockets[id];
            var msg = consume_js_object(text);
            if (s && s.ws && s.ws.readyState === 1) s.ws.send(msg);
        };
        importObject.env.vx_ws_next = function (id) {
            var s = sockets[id];
            if (!s || s.queue.length === 0) return -1;
            return js_object(s.queue.shift());
        };
        importObject.env.vx_ws_close = function (id) {
            var s = sockets[id];
            if (s && s.ws) s.ws.close();
            delete sockets[id];
        };
        importObject.env.vx_fetch = function (url) {
            var id = nextFetch++;
            var f = { done: false, res: null };
            fetches[id] = f;
            fetch(consume_js_object(url))
                .then(function (r) {
                    if (!r.ok) throw new Error("HTTP " + r.status);
                    return r.arrayBuffer();
                })
                .then(function (b) { f.res = { ok: 1, d: new Uint8Array(b) }; f.done = true; })
                .catch(function (e) { f.res = { ok: 0, d: String(e) }; f.done = true; });
            return id;
        };
        importObject.env.vx_fetch_poll = function (id) {
            var f = fetches[id];
            if (!f || !f.done) return -1;
            delete fetches[id];
            return js_object(f.res);
        };
        importObject.env.vx_page_url = function () {
            return js_object(window.location.href);
        };
        // Measurements for the comparison tooling (tools/ab), read by the
        // test browser from window.vertixMetrics.
        importObject.env.vx_report = function (json) {
            window.vertixMetrics = JSON.parse(consume_js_object(json));
        };
    }

    miniquad_add_plugin({ register_plugin: register_plugin, version: 1, name: "vertix_net" });
})();

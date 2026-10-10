# KRP desktop wrapper

A desktop window for KRP's browser client. It opens the client from a
reconstruction server, the same page a browser gets, in the system webview
(WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux), using the
libraries Tauri is built on (wry and tao).

No KRP code is built into it. The client is loaded from the server at run
time, and `inject.js` runs in the page before the client's own scripts. It
changes behaviour only by wrapping browser APIs the client calls:

- **Input rate.** KRP draws and sends one input per frame from a single
  `requestAnimationFrame` loop. `--input 60` caps that loop at 60 runs a
  second, so a fast display sends no more than 60 inputs a second. The
  default, `frame`, leaves it as KRP has it.
- **Sharp display.** KRP sizes its game canvas in CSS pixels, which a
  high-DPI screen upscales. With `--display sharp` (the default) the canvas
  is drawn at the device pixel ratio while still reporting CSS pixels to
  KRP. `--display krp` leaves it as KRP has it.
- **Metrics.** Frame times, inputs and server updates per second and ping,
  in the same JSON as the Rust client (`crates/client`), for
  `scripts/ab_compare.py`.

The same `inject.js` measures KRP's client in a browser for the comparison.

## Building

```bash
cargo build --release --manifest-path desktop/krp/Cargo.toml
```

On Linux this needs WebKitGTK's development files (for example
`libwebkit2gtk-4.1-dev` on Debian and Ubuntu). It is kept out of the Cargo
workspace for that reason.

## Running

Start the server with KRP's client built (`scripts/build-client.sh`), then:

```bash
desktop/krp/target/release/vertix-krp-desktop --room DEV0
```

| Option | Meaning |
| --- | --- |
| `--server URL` | Server address; defaults to this machine on port 8080, or `VERTIX_SERVER` |
| `--room NAME` | Room to join, as KRP's `/?ROOM` address does |
| `--name NAME` | Player name for the start menu |
| `--input frame\|HZ` | One input per frame (KRP) or a cap in Hz |
| `--display sharp\|krp` | Device-pixel canvas (default) or KRP's CSS-pixel canvas |
| `--autoplay` | Press ENTER GAME once the room is joined |
| `--script KEYS` | Scripted keys, e.g. `d:1500,s:800,a+w:1000,:500` |
| `--duration S` | Quit after S seconds and report metrics |
| `--metrics PATH` | Where to write the metrics JSON (stdout otherwise) |

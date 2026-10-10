# vertix_client

KRP's browser client (pinned commit 1e302cb, the 2019 v3.8 client) ported
to Rust with [macroquad](https://github.com/not-fl3/macroquad). The same
code builds for the browser (WebAssembly) and as a desktop build, and
talks to the reconstruction server like KRP's client does (socket.io over
WebSocket). Prediction uses the server's own rules from `crates/sim`.

The game code follows KRP's `app.ts` function by function, so the two can
be read side by side; `src/game/` mirrors its drawing, events, HUD and
start menu.

## Building

Browser build, served by the server at `/rust/`:

```bash
scripts/build-client.sh          # once: KRP's client, res.zip and font
rustup target add wasm32-unknown-unknown
scripts/build-rust-client.sh     # writes client/dist/rust/ (not committed)
cargo run --release              # then open /rust/ on the address it prints
```

Desktop build:

```bash
cargo run -p vertix_client --release -- --server http://SERVER:8080
```

## Launch options

Desktop takes `--key value`; the browser build takes `?key=value` in the
address.

| Option | Meaning |
| --- | --- |
| `server` | Server address (desktop; defaults to this machine on port 8080, or `VERTIX_SERVER`. The browser build uses its own page's) |
| `room` | Room to join, e.g. `DEV0` |
| `name` | Player name |
| `class` | Class index from the loadout |
| `input` | `frame` (KRP: one input per drawn frame, the default) or a fixed rate such as `60` (Hz) |
| `display` | `sharp` (default, full device resolution) or `krp` (KRP's CSS-pixel canvas, upscaled) |
| `floor` | `2016` (default: the 2016 ground tiles from the server's `original-2016` mod pack, when it has one) or `krp` (KRP's `res.zip` tiles) |
| `effects` | `persist` (default: blood, dust and bullet holes stay where they happened, even off screen) or `krp` (only effects that were on screen when they happened, dropped when they leave it) |
| `autoplay` | Skip the start menu and join straight away |
| `script` | Scripted input for tests, e.g. `d:1500,s:800,a+w:1000,:500` (keys and milliseconds) |
| `duration` | Seconds to run, then report metrics and quit |
| `metrics` | Desktop: file to write the metrics JSON to (stdout otherwise). The browser build sets `window.vertixMetrics` |
| `screenshot`, `shot-at` | Save a screenshot after `shot-at` seconds (default 5) |

The metrics JSON holds frame count, mean fps, frame time mean, p50, p95,
p99 and max, inputs and server updates per second, ping, device pixel
ratio and canvas size. It is what the client comparisons are built on.

## Differences from KRP's client

- The display is sharp by default: the world is drawn at the device's
  pixel ratio. `display=krp` restores KRP's CSS-pixel canvas.
- A fixed input rate is available (`input=60`); KRP sends one input per
  drawn frame, which is the default here too.
- The HUD, chat and start menu are drawn in the engine from KRP's
  `main.css` values instead of HTML, so fonts and spacing are close but
  not identical.
- KRP quirks are kept on purpose, among them: a flipped sprite shares its
  original's shadow, the minimap divides both tile sides by the map
  width, and the ammo-full check only looks at the first weapon slot.

## Not ported yet

Account, mods, the cosmetics loadout, the settings and controls screens,
custom rooms, the leaderboards page and sound (the stock game is silent).

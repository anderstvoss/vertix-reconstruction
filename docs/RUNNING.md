# Running it yourself

This runs the reconstruction server with KrunkerRevival's browser client on
your own machine. It works the same on Windows, macOS and Linux.

## What you need

- **Rust.** Install `rustup` from <https://rustup.rs>. The right toolchain
  (pinned in `rust-toolchain.toml`) installs itself on the first build.
- **Git, Node.js and pnpm**, to build the client once
  (<https://nodejs.org>, then `npm install -g pnpm`).
- **The private archive** (`vertix-archive`) with Git LFS files pulled, for
  the maps. The server reads
  `vertix-preservation/derived/maps/krp-2026-candidates/map-*.genData.json`
  and checks each against `vertix-preservation/manifests/sha256sums.txt`.
  Without the archive, switch `[maps] sources` to `["files"]` to play our
  own placeholder arena.

## Build the client

From the repository root:

```bash
scripts/build-client.sh
```

On Windows PowerShell:

```powershell
scripts\build-client.ps1
```

This clones KRP at the pinned commit into `client/krp`, builds its client
and copies the result to `client/dist` (both git-ignored). Options:

- `--source PATH` (`-Source`) clones from the archive's mirror
  (`vertix-preservation/mirrors/KrunkerRevivalProject-vertix.git`) instead
  of GitHub.
- `--res-zip FILE` (`-ResZip`) serves another `res.zip`, for example the
  2020 capture from the archive, instead of KRP's.

Run it again after changing either option; it reuses the checkout.

## Start the server

```bash
cargo run --release -- --archive PATH/TO/vertix-archive
```

Quote a Windows path that contains spaces: `--archive "PATH\TO\vertix archive"`.

When it is ready it prints the rules and balance preset in use, the maps
it loaded and the address it is listening on. Open that address, pick a
room in the server browser or press Play, and open a second window for a
second player.

Useful options:

- `--port 9000` to use another port. The bind address and port are in
  `config/server.toml`.
- `--trace out/trace.jsonl` writes every event in and out, one JSON line
  each.
- `VERTIX_ARCHIVE` replaces `--archive`.
- `--explain-rules` prints every rule constant and every balance value with
  its status or basis and its source, then exits.

## Cosmetics and mod packs

With an archive, the server serves the recovered hats, camos, sprays and
shirt image over the client build's copies, and the community mod packs at
`/mods/` (see [CONTENT.md](CONTENT.md)). Most packs live in a git bundle in
the archive; unpack them once:

```bash
python3 scripts/extract_mods.py --archive PATH/TO/vertix-archive
```

The start-up log says how many files and packs it restored. Load a pack by
typing its key (listed at `/mods/`) in the MODS tab; a client built before
this change needs `scripts/build-client.sh` again for keys to load locally.

## The 2016 client

With an archive, the server also serves the archived 2016-08-06 client on
a second port, from the archive's files (hash-checked, never copied here):
open `http://<bind>:8081/`. No client build is needed for it. Its players
join the room `[classic] room` names (the first room if empty), together
with players on KRP's client. Set `[classic] enabled = false` to turn it
off.

`python3 scripts/e2e_boot.py --url http://<bind>:8081/` checks it in a real
browser (needs Playwright): two players join, move and see each other, and
no request leaves the server.

## Rooms, balance and rules

`config/server.toml` holds:

- **`[game] rooms`**: the rooms opened at start, one per mode by default
  (`DEV0` free for all to `DEV8` Arsonist War, as in KRP's dev server). The
  room list and `/api/getIP` follow it. A room's mode changes at round end
  by vote, or through the client's custom server form.
- **`[game] max_players`**: players per room, 8 by default. A room can set
  its own (`{ name = "DEV0", mode = "ffa", max_players = 12 }`). The
  custom server form can lower a room's limit but not raise it.
- **`[game] balance`**: the balance preset laid over KRP's classes and
  weapons. `best` (default) uses the best-supported recovered value for
  each stat, `krp` keeps KRP's numbers, and a version such as `v3.8` plays
  that version's numbers, hiding classes that did not exist yet. The
  presets and their sources are in `data/balance/`.
- **`rules`**: layers of server constants (spawn protection, pickups,
  hardpoint timing, chat length, tick rate), merged in order, value by
  value. To try something without touching committed files, add your own
  layer at the end:

  ```toml
  [layer]
  status = "DECIDED"
  source = "local test"

  [rules]
  spawn_protection_ms = 1000

  [net]
  update_hz = 120
  ```

- **`[maps]`**: map sources, in order; a later source replaces a map with
  the same id. `"archive"` loads every `map-<id>.genData.json` in
  `archive_dir`, `"files"` loads the text maps listed under `files`. Each
  mode plays the ids its `maps` list names in `data/krp/gamemodes.json`, or
  any loaded map if none of those is loaded.

To let another machine on your network join, change `bind` to your
machine's network address (do not commit that change) and open the port
in your firewall.

## Checking it

```bash
cargo build && python3 scripts/e2e_smoke.py
```

starts the server with a stand-in client and the placeholder arena, and
plays through it over long-polling and WebSocket: room list, joining a
room, spawning, moving, chat after the WebSocket upgrade, a second player
and leaving.

## Troubleshooting

- **"hash mismatch" or "is an LFS pointer"** at start-up: that archive file
  was not pulled. Run `git lfs pull` in the archive.
- **Fewer cosmetic files or mod packs than expected** at start-up: the
  APK, Wayback files or JeanPaulDot bundle were not pulled (`git lfs pull`
  in the archive), or `scripts/extract_mods.py` has not been run.
- **"no client build"** warning: run `scripts/build-client.sh`. The 2016
  client on the second port works without it.
- **"The system cannot find the path specified"** for the maps: the
  archive clone predates the map files. Run `git pull` and `git lfs pull`
  in it.
- **The page loads but no room joins:** check the server log; the client
  connects to the same address the page came from.

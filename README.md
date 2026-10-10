# Vertix.io Reconstruction

A Rust reconstruction of Vertix.io's game mechanics, played in the browser.

> **Status:** early WIP. The server ports the game rules of
> [KrunkerRevival](https://github.com/KrunkerRevivalProject/vertix) (KRP)
> and serves KRP's browser client: rooms, spawning, movement, shooting,
> explosions, kills, assists, KRP's nine modes, pickups, round end and
> mode votes. See [docs/PLAN.md](docs/PLAN.md)
> for what comes next, [docs/DEVIATIONS.md](docs/DEVIATIONS.md) for what
> differs from KRP and from the original, and [CHANGELOG.md](CHANGELOG.md).

## Where things come from

- **Rules and feel: KrunkerRevival.** KRP (`KrunkerRevivalProject/vertix`,
  commit `1e302cb`) is the reference this server is ported from, so that the
  game feels the same. Its classes, weapons, modes and cosmetic catalogues
  are converted to `data/krp/` by `scripts/import_krp.py`, and the server
  logic in `src/game/room.rs` and `crates/sim/src/projectile.rs` follows its
  `server/room.ts`, `server/game.ts` and `core/src/logic/projectile.ts`.
  Credit for that work goes to the KRP contributors.
- **Numbers: the research.** Balance presets in `data/balance/` lay the
  values recovered for each game version over KRP's numbers. `best` (the
  default) takes the best-supported value for each stat; `krp` keeps KRP's
  own; `v0.20` to `v3.8` follow one version. Every value records its basis
  and source.
- **The client: built locally.** `scripts/build-client.sh` (or `.ps1` on
  Windows) builds KRP's client from a pinned commit into `client/dist`.
  Nothing of the client is committed here, and never any original
  Vertix.io file (`app.js`, `res.zip`, the Android APK, sprites or sounds).
- **Maps: read at run time.** By default the server plays the 24 map
  candidates from a local archive clone, each checked against its recorded
  SHA-256 before use, or our own text maps.

## License

[AGPL-3.0-only](LICENSE). This covers the code in this repository only, not
the original game, its client or its assets.

## Setup

After cloning, run once:

```bash
cargo install cargo-deny cargo-audit
git config core.hooksPath .githooks
```

`pre-commit` must be installed (pipx or pip, see <https://pre-commit.com/#install>).
The hooks run gitleaks, the template's blockers for keys, env files, local
paths and private IPs, a blocker for original game files, and
`scripts/sanitize_scan.py` (e-mails, local paths, private IPs and the
personal identifiers you list in the git-ignored `.sanitize-denylist`).
Set `VERTIX_ARCHIVE` to your archive clone so the scan can also confirm
that no tracked file is byte-identical to an archive original.

## Running

```bash
scripts/build-client.sh            # once: needs git, Node.js and pnpm
cargo run --release -- --archive PATH/TO/vertix-archive
```

and open the address it prints. Settings (bind address, port, rooms,
balance preset, rule layers, map sources) are in `config/server.toml`;
`--port`, `--trace out/trace.jsonl` and the `VERTIX_ARCHIVE` variable
override it, and `--explain-rules` prints every rule and balance value with
its source. Step-by-step instructions, including Windows, are in
[docs/RUNNING.md](docs/RUNNING.md).

With an archive, the archived 2016-08-06 client is also served, unmodified
apart from its version label, on a second port (`[classic]`, 8081 by
default). Its players join a KRP room (the first by default) alongside
KRP's client; the events that changed between the two clients are
translated in `src/classic/adapt.rs`.

A Rust port of KRP's client lives in `crates/client`. It builds for the
browser (`scripts/build-rust-client.sh`, served at `/rust/`) and as an
optional desktop build, and joins the same rooms as the other clients; see
[crates/client/README.md](crates/client/README.md).

`scripts/e2e_smoke.py` starts the server and plays through it over
long-polling and WebSocket, with no archive or client build needed.

## Development

```bash
cargo build
cargo test --workspace
```

Local gates (also run in CI once the repository is public):

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
python3 scripts/e2e_smoke.py
python3 -m unittest discover -s scripts/tests
gitleaks detect
```

Before publishing anything, run the full-history scan:

```bash
scripts/deep-scan.sh
```

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).
The hardening procedure this repository follows is in
[docs/REPO-SETUP.md](docs/REPO-SETUP.md) and
[docs/HARDENING-CHECKLIST.md](docs/HARDENING-CHECKLIST.md); the
pre-publication checklist is [GOING_PUBLIC.md](GOING_PUBLIC.md).

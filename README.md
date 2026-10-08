# Vertix.io Reconstruction

Runs the original 2016 Vertix.io browser client against a local,
reverse-engineered server, so the game can be played and studied again.

> **Status:** early WIP. The 2016-08-06 client boots and two players
> can join and move via the M1 Rust server. Combat and rounds are **not yet
> connected to that server**. This integration branch also preserves an
> experimental Python PvP prototype, a standalone Rust gameplay model and
> an opposite-key input shim, but these are not integrated into the M1 boot
> path. See [docs/GAMEPLAY-BRANCH-STATUS.md](docs/GAMEPLAY-BRANCH-STATUS.md),
> [docs/DEVIATIONS.md](docs/DEVIATIONS.md), and [docs/PLAN.md](docs/PLAN.md).

## What is and isn't in this repository

This repository holds only new code: the compatibility server, build and
fetch scripts, and small shims. **It never contains original Vertix.io
files** (the client's `app.js`, `res.zip`, the Android APK, sprites or
sounds), and never a modified copy of them. At run time the build reads
those files from a local copy of the archive, or fetches them from the
Wayback Machine, and checks every one against a recorded SHA-256 before
using it.

The first target build is the 2016-08-06 web client with the assets from
the August 2016 Android release.

The server's game rules are a reconstruction. The original server code was
never published, so anything the client cannot show us (damage, hit rules,
spawn logic, score limits) is a documented assumption, not recovered
behaviour.

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

You need a clone of the private archive with its LFS files pulled (the
2016-08-06 client, its page capture and the August 2016 APK). Then:

```bash
cargo run --release -- --archive PATH/TO/vertix-archive
```

and open the address it prints. The server checks every original against
`data/boot/20160806061006.json` before serving anything and refuses to
start on a mismatch. The bind address, port, map and assumptions file are
in `config/server.toml`; `--port`, `--trace out/trace.jsonl` and the
`VERTIX_ARCHIVE` variable override it.

Step-by-step instructions, including Windows, are in
[docs/RUNNING.md](docs/RUNNING.md).

`--trace` writes every event in and out as JSON lines, which is how the
protocol behaviour is checked against the client.

The browser check plays the game in Chromium with every outside request
blocked:

```bash
python3 -m pip install playwright
python3 scripts/e2e_boot.py --url http://HOST:PORT/ [--chromium PATH]
```

`data/boot/` and `data/contracts/` are derived, code-free facts from the
research repository, regenerated with `scripts/import_research.py`.

## Experimental gameplay components (this integration branch)

- `src/gameplay.rs` contains a **standalone**, tested PvP reference model
  with provisional damage, ammunition, reload deadlines and kill handling.
  The M1 Rust transport currently drives `src/game/mod.rs`, not this model.
- `web/pvp_server.py` is an **alternative**, experimental Python networked
  PvP prototype with synthetic-protocol tests, not the main Rust server.
- `web/input-opposites.js` corrects simultaneous opposing movement keys in
  the original client. It is currently served through the experimental
  Python boot shell; the M1 Rust page path has **not** injected it yet.
- Run the standalone regressions with
  `node --test scripts/tests/input-opposites.test.cjs` and
  `python3 -m unittest discover -s scripts/tests -p 'test_*.py'`.

The gameplay prototypes are preserved for consolidation; passing their
synthetic protocol tests does **not** demonstrate a complete authentic-client
PvP round. No archived game assets are checked in.

## Development

```bash
cargo build
cargo test
```

Local gates (also run in CI once the repository is public):

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
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

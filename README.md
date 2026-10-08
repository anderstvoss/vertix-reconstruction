# Vertix.io Reconstruction

Runs the original 2016 Vertix.io browser client against a local,
reverse-engineered server, so the game can be played and studied again.

> **Status:** WIP, not yet playable. The isolated branch has a hash-verified
> original-client HTTP bootstrap, a non-networked Rust gameplay simulation,
> and an opposite-key input compatibility shim, plus an experimental single-origin
> Engine.IO 3 polling/Socket.IO 1.x Python PvP runner. **Full browser PvP is
> not verified and projectile/map physics remain incomplete.** See
> [docs/GAMEPLAY-BRANCH-STATUS.md](docs/GAMEPLAY-BRANCH-STATUS.md),
> [docs/PLAN.md](docs/PLAN.md), and [CHANGELOG.md](CHANGELOG.md).

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

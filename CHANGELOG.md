# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0][keep-a-changelog], and
this project adheres to [Semantic Versioning 2.0.0][semver].

## [Unreleased]

### Added

- Repository created from the hardened baseline template, with an original-game-file blocker and a sanitization scan for e-mails, local paths, private IPs and personal identifiers.
- Menu footer shows our own version (`RECON 0.1.0`) instead of the original `V3.0`, a decided deviation.
- Compatibility server (`vertix-server`): Engine.IO 3 long-polling with binary and base64 payloads, Socket.IO 1.x events, `/getIP`, and the 2016-08-06 client served from a local archive with every file hash-checked.
- Joining, spawning, movement with the client's own collision, weapon swap, jumping and leaving, for any number of players in one free-for-all room.
- Placeholder map and an assumptions file for every number no source recovers.
- Event trace (`--trace`) and a browser check (`scripts/e2e_boot.py`).
- Game rules as ordered layers (`data/rules/`), each value with a status and source; `--explain-rules` lists them and start-up logs a hash of the merged rules.
- The seven 2016 modes with their score limits, team flag and map lists; team modes balance red and blue and fill the team score bars, and Sniper War and Rocket War force Hunter and Rocketeer.
- Maps as data from swappable, combinable sources: every map file in an archive directory (by default the 24 provisional maps, hash-checked), or our own text maps; modes name their map ids in the rule layers. Spawns use the maps' red and blue cells.
- Server tick rate is a setting (`net.update_hz`).
- POST bodies with more than 256 Engine.IO packets are rejected, and the codec has tests for malformed length prefixes and multibyte text.

- KrunkerRevival (KRP) port: the server's game logic follows KRP's `room.ts`, `game.ts` and projectile model (rooms, movement, shooting, explosions, kills, assists, nine modes, pickups, hardpoints, zones, round end, mode votes, chat, likes, sprays, custom server form). KRP is credited in the README and docs.
- The 2016-08-06 client kept as a compatibility path on its own port (`[classic]`): it is served from the archive and joins a KRP room through an event translation layer (`src/classic/adapt.rs`), checked against the client's recovered event contract and by `scripts/e2e_boot.py` in a browser.
- KRP's classes, weapons, modes and cosmetics as data (`data/krp/`, converted by `scripts/import_krp.py`).
- Balance presets from the research (`data/balance/`): `[game] balance` picks `best` (default), `krp`, or one game version; `--explain-rules` lists the values and their sources.
- Serves a locally built KRP client (`scripts/build-client.sh`, `.ps1`) with KRP's `/api/getIP`, `/api/getRooms` and `/api/getLbs` routes.
- Rooms from `config/server.toml`, one per mode by default.
- WebSocket transport with the Engine.IO upgrade.
- `scripts/e2e_smoke.py`: plays through a running server over polling and WebSocket.

### Changed

- Engine.IO 4 / Socket.IO 5 with one namespace per room for KRP's client; Engine.IO 3 now serves only the 2016 client's port.
- Server tick 60 Hz; bullets advance on it, and pickups, hardpoints and zones are checked on it, so hardpoints keep scoring while a player's tab is hidden (KRP checks them only on input).
- Rule layers now hold only server constants (`base.toml`, `recovered.toml`); class, weapon and mode numbers come from the KRP data and the balance preset.

### Deprecated

### Removed

- `data/assumptions.toml`, replaced by the rule layers.

### Fixed

### Security

[keep-a-changelog]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/anderstvoss/vertix-reconstruction/compare/HEAD...HEAD

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
- Maps from a swappable source: the 24 provisional maps read and hash-checked from the archive, or our own text maps. Spawns use the maps' red and blue cells.
- Server tick rate is a setting (`net.update_hz`, 30 by default).
- POST bodies with more than 256 Engine.IO packets are rejected, and the codec has tests for malformed length prefixes and multibyte text.

### Changed

### Deprecated

### Removed

- `data/assumptions.toml`, replaced by the rule layers.

### Fixed

### Security

[keep-a-changelog]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/anderstvoss/vertix-reconstruction/compare/HEAD...HEAD

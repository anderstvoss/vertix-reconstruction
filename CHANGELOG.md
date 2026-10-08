# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0][keep-a-changelog], and
this project adheres to [Semantic Versioning 2.0.0][semver].

## [Unreleased]

### Added

- Repository created from the hardened baseline template, with an original-game-file blocker and a sanitization scan for e-mails, local paths, private IPs and personal identifiers.
- Compatibility server (`vertix-server`): Engine.IO 3 long-polling with binary and base64 payloads, Socket.IO 1.x events, `/getIP`, and the 2016-08-06 client served from a local archive with every file hash-checked.
- Joining, spawning, movement with the client's own collision, weapon swap, jumping and leaving, for any number of players in one free-for-all room.
- Placeholder map and an assumptions file for every number no source recovers.
- Event trace (`--trace`) and a browser check (`scripts/e2e_boot.py`).

### Changed

### Deprecated

### Removed

### Fixed

### Security

[keep-a-changelog]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/anderstvoss/vertix-reconstruction/compare/HEAD...HEAD

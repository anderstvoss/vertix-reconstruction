# Plan

Milestones, in order. Each one ends with a reproducible command and
records what was tested and what was not. The direction is in
[DEVIATIONS.md](DEVIATIONS.md#direction-2026-10-09): port KrunkerRevival
(KRP) to Rust so the game feels the same, then refine it with the research.

## 1. Port KRP's server (done)

- **Transport.** Engine.IO 4 / Socket.IO 5, long-polling with WebSocket
  upgrade, one namespace per room, as KRP's client connects.
- **Game.** Rooms, spawning, movement, jumping, shooting with KRP's
  projectile model, explosions, damage, kills, assists, kill streaks, all
  nine modes, healthpacks, loot crates, hardpoints, zones, round end, mode
  votes, chat, likes, sprays and the custom server form.
- **Data.** KRP's classes, weapons, modes and cosmetics in `data/krp/`,
  with the balance presets from the research laid over them.
- **Client.** KRP's client, built locally from a pinned commit.
- **Tested:** unit tests for every rule above, and `scripts/e2e_smoke.py`
  over polling and WebSocket. **Not yet:** a full session in a real browser
  with the built client.

## 2. Play-test against KRP

Play the same scenarios on KRP and on this server and compare: movement,
hit registration, explosions, round flow. Fix what feels different.

## 3. Client refinements

Patches applied by the build script, never committed copies of the
client: the `RECON` version label, a fixed input rate, and the
`devicePixelRatio` fix (research #72).

## 4. Local save and content

A local save with every cosmetic unlocked, behind a swappable interface.
Then restore what the research recovered: cosmetics, mods and maps.

## Notes

- The template's `block-local-network-targets` hook rejects loopback
  addresses anywhere in the tree. The default bind address lives in
  `config/server.toml` alone, which has a narrow `exclude`.
- Deviations from KRP and from the original are recorded in
  [DEVIATIONS.md](DEVIATIONS.md) as they are made.

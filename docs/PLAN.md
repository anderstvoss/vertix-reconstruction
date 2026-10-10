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
- **Client.** KRP's client, built locally from a pinned commit. The
  archived 2016 client stays as a compatibility path on its own port,
  joining the same rooms through an event translation layer.
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

## 5. Android 0.0.3 historical-client compatibility (after the browser baseline)

- **Source identity.** Use the preserved authentic `tbs.vertix.io` v0.0.3
  Cordova APK; the unrelated Xamarin project with the same name is **not**
  a Vertix.io game source. Verify the APK hash from the private archive.
- **Android bootstrap.** Redirect the original mobile discovery-script request
  to a local test callback. The exact historical callback response is unknown,
  so generated responses are compatibility fixtures, not recovered bytes.
- **Protocol and input.** Test the real packaged Socket.IO 1.x client,
  including the mobile-specific session event, analog input magnitude,
  and fifth firing argument. The identical event names/arity alone do not
  prove identical movement or weapon behavior.
- **Done when:** one unmodified web client and one unmodified Android APK
  share an authoritative match and complete movement, combat, death, respawn
  and a full round. Any patched APK/runtime result is recorded separately.
- **Tracking:** [issue #9](https://github.com/anderstvoss/vertix-reconstruction/issues/9);
  [research protocol comparison](https://github.com/anderstvoss/vertix-research/blob/main/analyses/ANDROID-WEB-WIRE-DIFF.md).

## Notes

- The template's `block-local-network-targets` hook rejects loopback
  addresses anywhere in the tree. The default bind address lives in
  `config/server.toml` alone, which has a narrow `exclude`.
- Deviations from KRP and from the original are recorded in
  [DEVIATIONS.md](DEVIATIONS.md) as they are made.

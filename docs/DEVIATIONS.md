# Deviations and inferences

Where this server knowingly behaves differently from the 2016 original, or
where its behaviour is our inference rather than recovered fact. Every
entry says which it is. Add to this list in the same change that makes the
deviation.

## Deviations (we chose to differ)

| Area | Original | Here | Why |
| --- | --- | --- | --- |
| Transport | Engine.IO offered a WebSocket upgrade | Long-polling only (`upgrades: []`) | Polling is what the client needs to run; WebSocket is a later milestone. The client accepts an empty upgrade list. |
| Page | Loaded jQuery and Socket.IO from public CDNs, plus ads, analytics and social widgets | The two CDN URLs are rewritten to `/cdn/<host>/<path>` and served from the hash-checked archive copies; a Content-Security-Policy header blocks every other third-party request | No request may leave the local server. These two URL substitutions and the version label (below) are the only changes to the page, and each must match exactly once or the server refuses to start. |
| Version label | Menu footer shows `V3.0 (CHANGELOG)`, linking the original changelog | Shows `RECON <our version> (CHANGELOG)`, linking this repository's changelog | Decided by Anders (2026-10-08): this build is not a faithful V3.0, so it must not claim to be. Done as a third page rewrite; `app.js` is untouched. |
| `/getIP` | Named a live game server | Answers with the host and port the browser used to reach us | Archived replies point at the original servers and are never served. |
| Lobbies | `create` with an argument joined or created a private lobby | Every `create` joins the one public room | Private lobbies are not built yet. |
| Abrupt disconnects | Same | A tab that vanishes without a close packet is dropped after `pingInterval + pingTimeout` (85 s) | Same timing as the original handshake; noted because tests must disconnect cleanly to see `rem` quickly. |
| Accounts | Login, stats and cosmetics from the original backend | Not implemented yet; every player is a guest | Decided by Anders (2026-10-08): no accounts. A local save with every unlockable owned replaces them, behind a swappable interface (next PR). |
| Anti-cheat | Client emits `kil` when it detects a minimap hack | Ignored (traced as unhandled) | No server-side behaviour is known. |

## Placeholders (no original data survives)

- **Map.** No original map survives. `data/maps/arena.txt` is our own
  layout in the client's map format. The map source is a file path in
  `config/server.toml`, so a better candidate can be dropped in.
- **Numbers.** Class and weapon values, tick rate, input clamp, spawn
  protection and the score limit are in `data/assumptions.toml`, each
  marked as recovered or assumed. Game code reads them from there only.

## Inferences (our reading of the client, to be confirmed)

- **Join sequence.** Play sends `create` (when not in a room) and
  `respawn`. We answer `welcome({id, room}, false)`; the client replies
  `gotit(player, flag, Date.now(), false)`; we then send `gameSetup` to the
  joiner, `add` to the others, `lb` and `ts`. `welcome` with `true` sends the
  client back to the menu, which we take to mean round end.
- **Snapshots.** `rsd` carries 6-value records `[6, index, x, y, angle, n]`
  where `n` is the last input sequence number for your own player (the
  client replays later inputs on top) and the name offset for others.
- **Movement.** Input `4` carries direction, timestamp and sequence. The
  server applies `delta = ts - previous ts`, clamped to
  `max_input_delta_ms`, and the client's own wall collision, so its
  prediction and the server agree.
- **Shooting gate.** The client only fires when `spawnProtection` is the
  number `0`, so we send `upd {i, sp: 0}` when protection ends.

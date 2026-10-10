# Deviations and inferences

Where this server knowingly behaves differently from its references, or
where its behaviour is our inference rather than recovered fact. Every
entry says which it is. Add to this list in the same change that makes the
deviation.

## Direction (2026-10-09)

The first milestone served the archived 2016-08-06 client unmodified,
against a server reconstructed from that client's handlers (Engine.IO 3,
long-polling, one room). Decided by Anders on 2026-10-09: this repository is
a Rust reconstruction of the game's mechanics, and it takes
[KrunkerRevival](https://github.com/KrunkerRevivalProject/vertix) (KRP) as
its baseline engine and server reference. KRP already plays well and its
client is far more refined, so the server is ported from KRP's
`server/room.ts`, `server/game.ts` and `core/src/logic/projectile.ts`, and
serves KRP's own client (built locally, never committed). The aim is that
the game feels the same as KRP. Numbers recovered by the research replace
KRP's where we have them, through the balance presets.

The 2016 client stays as a compatibility path (asked for by Anders,
2026-10-09): its Engine.IO 3 transport, the archived page served from the
archive, and the boot manifest and browser check are kept, on their own
port. Its players join one of the KRP rooms, and `src/classic/adapt.rs`
translates the events that changed between the two clients. Credit for
the game logic and data this port follows goes to the KRP contributors.

## The 2016 client on KRP rooms (compatibility path)

Each difference below was RECOVERED from the 2016-08-06 client's handlers;
the translation is INFERRED to be what its server did.

| Event | 2016 client | KRP room | Translation |
| --- | --- | --- | --- |
| Joining | Root namespace; `create`, then `respawn` | One namespace per room | `create` (or the first `respawn`) seats the client in `[classic] room`; KRP's menu `welcome` at join is not sent. Lobby keys are ignored. |
| `4` input | Carries a timestamp, no frame delta | Carries the frame delta | The delta is the gap between timestamps, capped at 100 ms. |
| `like` | Names the target only | Names liker and target | The sender is the liker. |
| `1` hit | `amount`, `bi`, `h` | `healthDelta`, `bulletIndex`, `health` | Renamed. |
| `upd` | `sp` a number, `l` a count | `sp` a bool, `l` a list | Converted. |
| `ts` | Bar widths in percent, or the score limit in free for all | Raw scores | Computed from the room's scores and the mode's limit. |
| `7` round end | Also carries the player list | No player list | The room's players are added. |
| `tprt` | `scor`, `oldX`, `oldY` | `score`, no old position | Renamed; the old position repeats the new one. |
| `yourRoom` | Also a server key | Room only | `host/room` is added. |
| `gameSetup` | The map only when it changed; pixels under `genData.data.data` | The map every time; pixels under `genData.data` | Tracked per client; pixels nested. |
| Players | `spawnProtection` number, `likes` count | `isSpawnProtected`, `likedBy` | Both sets of fields are sent. |
| Shirts, accounts, lobbies | No shirts; accounts and lobbies on the server | Shirts; neither | `updShrt` is not sent; account (`db*`), `kil` and `5` messages are ignored. |

## Deviations from KRP (we chose to differ)

| Area | KRP | Here | Why |
| --- | --- | --- | --- |
| Numbers | Its own class and weapon values | A balance preset is laid over them (`[game] balance`, `best` by default; `krp` restores KRP's) | Recovered values win where the research has them (issue #22). |
| Bullet timing | Each bullet advances on its own timer, by the shooter's last frame delta | Every bullet advances on one fixed server tick (`net.update_hz`, 60) | One clock for the whole room; the client's frame rate no longer changes bullet speed on the server. |
| Movement input | Moves by the frame delta the client reports, unchecked | The same, with the delta capped at 100 ms | A client cannot move further than one slow frame allows. |
| Pickups, hardpoints and zones | Checked only when the client sends input, and the hardpoint interval counts down by the client's frame delta, so a player whose tab is hidden stops scoring (reported by Anders while playing KRP, 2026-10-09) | Checked for every living player on the server tick; the interval counts down by server time | Scoring follows time on the point, not the client's frame rate or tab focus. |
| Round restart | Sends every player's `welcome` to everyone, so each client ends up with the last player's id | Each player gets their own `welcome` | Bug fix. |
| Custom server modes | Vote entries for a custom mode list are numbered by list position, so the next round can start the wrong mode | Entries keep the mode's real index | Bug fix. |
| Custom server form | Player count, multipliers and modes used as sent | Players clamped to 2 to the room's configured limit, multipliers to 0.01 to 100, unknown modes dropped, numbers accepted as text | Research #69: the form sends unchecked strings. |
| Likes | The liker's index is taken from the message | The liker is always the sender | One player cannot like on another's behalf. |
| Chat and names | Used as sent | Markup and control characters stripped; chat capped at 50 characters, names at 25 | Rendered by every client. |
| Rooms | Unknown names in `/api/getIP` fall back to the first room | The same, and connecting straight to an unknown room namespace is refused | Rooms come from `config/server.toml` only. |
| Player limit | 8 per room, fixed in code | `[game] max_players` in `config/server.toml`, 8 by default, and a room may set its own | Requested by Anders, 2026-10-09: a server setting. |
| Leaderboards | `/api/getLbs` returns generated sample players | Every board is empty | There are no accounts (decided by Anders, 2026-10-08). |
| `/api/getIP` | Names a fixed host and port | Answers with the host and port the request used | The client connects to its own origin either way. |
| Boss class | Selectable like any class | Reserved for the boss in Boss mode even when a balance version hides it | The boss must spawn as the boss. |
| Cosmetic art | Its fan-repository copies of every hat, shirt, camo and spray | First-party copies from the archive where they exist (`data/content/cosmetics.json`), KRP's for the rest | Recovered assets override KRP's (Anders, 2026-10-09). Sprays 1 to 43 keep their 2016 sizes instead of KRP's 30 px re-sizes. |
| Mod packs | A mod key goes to Dropbox (dead links); a path such as `/mods/x/vertixmod.zip` becomes `http:///mods/...` and fails; no packs shipped | Keys and paths load from this server, which serves 21 community packs from the archive (`/mods/`); a client patch applied at build time (`scripts/client-patches/apply.mjs`) | Mods work offline; the stock game is silent, so packs are the only sound. |

## Player-expectation changes (not faithful)

Changes made on purpose because players of a modern multiplayer game
expect them, even though neither the original nor KRP behaved this way
(direction from Anders, 2026-10-10). Each is marked here so a faithful
mode can restore the original behaviour if wanted.

| Area | Original and KRP | Here | Why |
| --- | --- | --- | --- |
| Weapon camos | KRP (the original server is not recovered) keeps one weapon object per room, so the last player to pick a camo for a weapon changes it on every player's copy of that gun | Camos are per player, like hats and shirts: each player carries their own weapons, and the server remembers each player's camo per weapon across respawns and class changes (the client sends it once, before it spawns) | A cosmetic should only change the player who chose it. |

## Not done yet

- **Version label.** KRP's client shows its own version; showing `RECON`
  (decided by Anders, 2026-10-08) needs a client patch
  (`scripts/client-patches/apply.mjs` now applies patches at build time).
- **Client frame and input rate.** KRP's client has no 30 fps limiter and
  sends input every frame (research #72). A fixed input tick and the
  `devicePixelRatio` fix are client patches still to come.
- **Accounts.** None, by decision. A local save with everything unlocked
  replaces them (next).
- **Leaderboard and profile pages.** KRP's build only bundles the main
  page.

## Placeholders (no original data survives)

- **Maps.** No original map file survives. By default the server plays the
  24 KRP map candidates from the archive (PROVISIONAL, decided by Anders on
  2026-10-09), read and hash-checked at start-up. The map list is data: the
  archive source loads whatever `map-<id>.genData.json` files its directory
  holds, `[maps] sources` can combine it with text maps such as our own
  `data/maps/arena.txt` (later sources replace maps with the same id), and
  each mode names its map ids. New map evidence means new files and ids,
  not code.
- **Server constants.** Spawn protection, pickup timing, hardpoint and zone
  scoring, the round-end countdown and similar values are KRP's
  (`data/rules/base.toml`, PROVISIONAL), each with its source, and can be
  overridden by a later rule layer.

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

The 2016 compatibility server (Engine.IO 3, the archived page, the boot
manifest and its browser check) was removed in the same change; it remains
in the history before it. Credit for the game logic and data this port
follows goes to the KRP contributors.

## Deviations from KRP (we chose to differ)

| Area | KRP | Here | Why |
| --- | --- | --- | --- |
| Numbers | Its own class and weapon values | A balance preset is laid over them (`[game] balance`, `best` by default; `krp` restores KRP's) | Recovered values win where the research has them (issue #22). |
| Bullet timing | Each bullet advances on its own timer, by the shooter's last frame delta | Every bullet advances on one fixed server tick (`net.update_hz`, 60) | One clock for the whole room; the client's frame rate no longer changes bullet speed on the server. |
| Movement input | Moves by the frame delta the client reports, unchecked | The same, with the delta capped at 100 ms | A client cannot move further than one slow frame allows. |
| Weapons | All players share the room's weapon objects, so a camo choice changes everyone's | Each player carries their own copy | Per-player camos. |
| Round restart | Sends every player's `welcome` to everyone, so each client ends up with the last player's id | Each player gets their own `welcome` | Bug fix. |
| Custom server modes | Vote entries for a custom mode list are numbered by list position, so the next round can start the wrong mode | Entries keep the mode's real index | Bug fix. |
| Custom server form | Player count, multipliers and modes used as sent | Players clamped to 2 to 8, multipliers to 0.01 to 100, unknown modes dropped, numbers accepted as text | Research #69: the form sends unchecked strings. |
| Likes | The liker's index is taken from the message | The liker is always the sender | One player cannot like on another's behalf. |
| Chat and names | Used as sent | Markup and control characters stripped; chat capped at 50 characters, names at 25 | Rendered by every client. |
| Rooms | Unknown names in `/api/getIP` fall back to the first room | The same, and connecting straight to an unknown room namespace is refused | Rooms come from `config/server.toml` only. |
| Leaderboards | `/api/getLbs` returns generated sample players | Every board is empty | There are no accounts (decided by Anders, 2026-10-08). |
| `/api/getIP` | Names a fixed host and port | Answers with the host and port the request used | The client connects to its own origin either way. |
| Boss class | Selectable like any class | Reserved for the boss in Boss mode even when a balance version hides it | The boss must spawn as the boss. |

## Not done yet

- **Version label.** KRP's client shows its own version; showing `RECON`
  (decided by Anders, 2026-10-08) needs a client patch in the build script.
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

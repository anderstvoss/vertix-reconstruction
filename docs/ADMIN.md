# Admin panel and dev console

The server has an admin panel: a page on its own port with a live view of
every room, buttons for the common actions, editable settings, and a
console that takes any command. The same commands work typed into the
server's terminal and through `POST /api/cmd`, so a script can drive a
test. None of this is in the original game or in KRP; it is a tool for
testing and hosting.

## Opening it

Start the server as usual. It prints the panel's address with a token:

```text
admin panel on http://<bind>:8082/#token=3f9c...
```

The port is a preference: if 8082 is taken (say, by a second server),
the next free port is used, and the line shows which. Open that address
in a browser on the same machine. The token is part of
the address after `#`, which the browser never sends to a server; the page
keeps it for the tab and removes it from the address bar. Without the
token the page only asks for it.

Settings, in `config/server.toml`:

```toml
[admin]
enabled = true      # serve the panel
bind = "..."        # loopback by default: only this machine can reach it
port = 8082
token = ""          # empty: a new random token at every start
stdin = true        # also read commands typed into the server's terminal
stdin_log = false   # print joins, chat and the kill feed in the terminal
```

Keep `bind` on loopback. If you open the panel to other machines, set a
long `token` and know who can reach the port: the panel can kick anyone,
end rounds and change every rule. The server warns at start-up when the
panel is reachable from other machines.

## The panel

- **Room tabs** (one per room, with mode and player count) and a
  **Server** tab. **+ Room** opens a new room (the next free `DEVn` name and
  a mode no room is playing, both changeable); the **×** on a tab closes
  that room after asking, disconnecting its players. The last room cannot
  be closed.
- **Round:** pick a mode and map (or random) and start a new round; restart;
  end the round with red, blue or nobody winning; set team scores and the
  score limit; pause the room's server tick.
- **Players:** kill all, kick all, refill pickups, resync positions; per
  player kill, kick (with a reason), win, lose, heal, spawn protection,
  score, rename and team swap.
- **Room settings:** player limit, health and speed multipliers, chat and
  announcements from the server.
- **Server tab:** balance preset, the room the 2016 client joins, and
  every rule constant, editable in place, with a
  reload from disk.
- **Console:** type any command below. `Tab` completes command names, the
  arrow keys recall earlier commands, `clear` empties the log. The log
  shows joins, leaves, chat, the kill feed, round ends and every command
  anyone ran, live.

## Commands

A command is words separated by spaces; quotes keep spaces in one word,
and a JSON array or object runs to the end of the line. `@ROOM` anywhere in
the line picks the room; otherwise the panel's selected room tab, the
terminal's `use` room, or the first room. A player is an index (`3` or
`#3`), a name, or a unique start of a name, ignoring case. A team is `red`
or `blue`; in free-for-all modes a player's team is their index.

| Command | Does |
| --- | --- |
| `help [command]` | List commands, or show one |
| `status` | Every room's mode, map, round and players |
| `state` | Full server state as JSON (what the panel shows) |
| `catalog` | Modes, maps, classes, weapons, presets as JSON |
| `players` | Players in the room |
| `list modes\|maps\|classes\|weapons\|hats\|shirts\|camos\|sprays\|presets\|versions` | List game data |
| `use <room>` | Work in this room from now on (this session) |
| `mode <mode> [map]` | Start a new round in a mode (code, name or index) |
| `map <map id>` | Start a new round on a map, same mode |
| `restart` | Start a new round now, same mode, random map |
| `win <team\|player\|none>` | End the round with this winner |
| `lose <team\|player>` | End the round with this team or player losing |
| `score <team\|player> <points>` | Set a score (can end the round) |
| `addscore <team\|player> <points>` | Add to a score (negative subtracts) |
| `scorelimit <points\|default>` | Score limit for this room |
| `pause` / `resume` | Stop or restart the room's server tick |
| `kick <player> [reason]` | Disconnect a player with a message |
| `kickall [reason]` | Disconnect everyone in the room |
| `kill <player>` | Slay a player (no score for anyone) |
| `killall` | Slay every living player in the room |
| `health <player> <hp>` | Set a living player's health |
| `protect <player> on\|off` | Spawn protection until their next spawn |
| `team <player> <team>` | Move a player to a team (slays them) |
| `rename <player> <name>` | Rename (others see it from next spawn) |
| `tp <player> <x> <y>` | Teleport (Zone War's teleport event) |
| `maxplayers <n>` | Players the room takes (up to 64) |
| `healthmult <x>` | Health multiplier, from next spawn |
| `speedmult <x>` | Speed multiplier, from next spawn |
| `pickups` | Make every health pack and loot crate available |
| `say <text>` | Chat line from the server |
| `announce <title> [text]` | Big centre text for every living player |
| `sync` | Resend everyone's positions |
| `rooms` | List rooms |
| `open <room> <mode>` | Open a new room |
| `close <room>` | Close a room (kicks its players) |
| `classic <room>` | Room the 2016 client's players join |
| `rule [name] [value]` | List, show or change a rule constant (all rooms) |
| `reload rules` | Reload the rule layers from disk |
| `balance [preset]` | Show or switch the balance preset (from next spawn) |
| `version` | Show the version string clients are given |
| `version use <version> [label]` | Use a researched game version's string (`v3.8` shows as `V3.8`) and its balance; `label` changes only the string |
| `version set <text>` | Any version string (up to 40 letters, digits, spaces and `.-_+:/#!,()`) |
| `version reset` | Back to the server's own (`RECON <version>`) |
| `tune` | List the values tuned over the preset |
| `tune <class\|weapon> <name> <field> <value>` | Set one class or weapon value, from next spawn |
| `tune show <class\|weapon> <name>` | A class's or weapon's current values |
| `tune reset` | Drop all tuning |
| `emit <player\|all> <event> [json args]` | Send any event to clients |
| `inject <player> <event> [json args]` | Handle an event as if a player sent it |

Examples:

```text
@DEV1 mode hp 7          # Hardpoint on map 7 in DEV1
lose red                 # blue wins this round
kick Bob "Please rename"
rule round_end_countdown_s 5
emit all 6 ["TEST", "big text", 1.25]
inject 0 cht ["hello from player 0"]
```

### What each action looks like to players

Every action uses an event the client already handles, so it looks like
something that happens in a normal game:

- `win`, `lose` and reaching a `score` show the round's end screen and the
  mode vote, and the next round starts after the usual countdown.
- `mode`, `map` and `restart` send everyone back to the class menu, as
  the start of a new round does.
- `kill` shows as a suicide and scores nothing; `team` kills the player so
  they respawn on their new team.
- `kick` shows the client's "disconnected" screen with the reason.
- `tp` uses Zone War's teleport, so the player sees its "zone entered"
  text.
- `pause` stops the server's tick for the room: bullets, timers, pickups
  and objectives stop, but players can still walk on their own screens.
- `healthmult`, `speedmult`, `balance` and `rename` apply when each player
  next spawns. `rule max_players` and `rule bullet_pool` apply to new rooms
  and new rounds; per-room limits are `maxplayers`.

## Version string

Both clients show a version in their menu footer, as `<version>
(CHANGELOG)`. The server writes its version string into that label as it
serves each client's page and scripts, and answers `/api/version` on the
game port. It starts as `[game] version` from the config, or the server's
own `RECON <version>`. In the panel's Server tab, the version list holds
every game version the research has a balance sheet for, with its date:
picking one gives clients that version's string and, unless you untick
"also load its balance", switches to its balance preset. A text box sets
any other string. Players are told in chat when it changes; menus show it
after a page reload.

KRP's client is matched by its built label `V3.8 (CHANGELOG)`; if a KRP
build words it differently the label is left alone.

## Balance tuning

`tune` sets any class value (`maxHealth`, `speed`, `jumpStrength`,
`gravityStrength`, `height`, `width`) or any number or flag of a weapon
(damage, reload, spread, bullet speed and the rest; `tune show weapon
<name>` lists them). Tuned values sit on top of the balance preset, so
switching preset or version keeps them; `tune reset` drops them. Like a
preset, they apply to each player from their next spawn. The panel's
Server tab has a picker for the class or weapon and the field, showing the
current value.

## Ports

The ports in `config/server.toml` (8080 game, 8081 2016 client, 8082
panel) are preferences. A taken port moves to the next free one up, then
to one the system picks; the listeners never share a port. Each line at
start-up shows the address actually used, and `ports_file`
(`out/server.json` by default) has them all for scripts:

```json
{"admin": "http://<bind>:8082/", "classic": "http://<bind>:8081/", "krp": "http://<bind>:8083/", "pid": 1234}
```

`strict_ports = true` or `--strict-port` fails instead of moving.

## From a script

```bash
curl -s -H "Authorization: Bearer $TOKEN" \
  -d '{"line": "status", "room": "DEV0"}' http://<bind>:8082/api/cmd
```

The answer is `{"ok": true, "text": "...", "data": ...}`. `data` carries
the structured result (for `state`, `catalog`, `players`, `rule` and
`list`). `scripts/e2e_smoke.py` uses this and the terminal console to test
kill, win, restart, kick and chat against a real player connection.

The panel's WebSocket (`/ws?token=...`) takes `{"id": 1, "line": "..."}`
and answers `{"type": "reply", "id": 1, "reply": {...}}`; it also sends
`{"type": "log", "line": {...}}` for each log line. It refuses pages from
other origins.

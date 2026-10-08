# Gameplay compatibility branch status — 2026-10-08

Branch: `work/gameplay-pvp-compat-2026-10-08`. Source: reconstruction `main` at
`9b078203ffd2d8eaa3583e7810e3afdb9b67a287`. Do not merge until
the authentic-client acceptance gates below pass.

## Repositories reviewed and evidence authority

- `anderstvoss/vertix-archive` `master`, `a059176db6671db6f35c1a3e2457495c08c846ac`:
  original 2016-08-06 client, resource APK, original-input behavior.
- `anderstvoss/vertix-research` `main`, `2f476d32ab9a2e3cf125fd2f33692b1e93fd601c`:
  `analyses/ARCHITECTURE-CONSTRAINTS.md`, `SERVER-CONTRACT.md`,
  `RELOAD-FORENSICS.md`, `STRICT-GAMEPLAY-PROFILE.md`,
  `RECONSTRUCTION-BASE.md`, `runtime/serve.py`,
  `runtime/two_client_probe.py`. The research harness does NOT prove
  production-quality server authority.
- `anderstvoss/vertix-reconstruction` `main`: Rust library-only scaffold
  at branch creation; no original-client transport or playable server.

Original client / APK binaries are not copied into this public repository.

## Implemented (new code, not original game code)

| Component | Files | Evidence / limits |
|---|---|---|
| Opposite-key behavior | `web/input-opposites.js` | Original key handler clears `keyMap` for the opposite key. New post-app listener restores physical held states and computes canceled directions. Does not modify original `app.js`. |
| Provenance-gated browser shell | `config/boot-20160806.json`, `web/serve.py` | Exact SHA-256 checks, ZIP APK-member support, HEAD `/res.zip`, local CDN URLs, local `/getIP` and shim after `app.js`; no original files committed. |
| Experimental socket/PvP runner | `web/pvp_server.py` | Pure-Python, loopback-only EIO3 Socket.IO 1.x polling, independent sessions, synthetic FFA map, `welcome`/`gameSetup`, `rsd`, hit/damage/kill events and targeted `r` completion. Experimental bridge is **not coupled** to the Rust reference core and must be consolidated. |
| Isolated gameplay model | `src/gameplay.rs` | Client-version-appropriate nine class/weapon mappings; per-slot reload deadlines and completion acknowledgements; per-player ammo, damage, kill, score and respawn; bounded movement inputs. **Standalone Rust simulation; no connected network adapter.** |
| Automated tests | `scripts/tests/*`, `.github/workflows/gameplay-validation.yml` | Rust model, synthetic EIO3 two-HTTP-session PvP wire regression, archive boot HTTP and VM input-event tests. Strict Rust fmt/Clippy gates. |

### Reload protocol

Source-derived: the original client emits `r` with zero positional arguments after beginning a reload; the server must respond with `r` carrying **one weapon-slot index**, routed **only to the client that reloaded**. Its handler then fills that slot's ammo and clears reloadTime. `Match::tick` returns `ReloadCompletion { player_id, slot }` at each deadline so the eventual adapter can deliver the acknowledgement exactly once. The actual reload durations are **provisional**, not recovered.

### Provisional balance, combat and limitations

The original authoritative server was not recovered. The `PROVISIONAL_WEAPONS`
table in `src/gameplay.rs` derives rough starter defaults from the KRP
community reconstruction and research overrides. It is **not an original
weapon-balance table**. The simulation currently uses instantaneous
nearest-hit hitscan, not historical projectile travel, obstacle occlusion,
ricochet, pierce, explosions, splash or lag compensation. No map-collision
engine, historical spawn selection, mode loop, anti-cheat, room multiplexing,
account backend or save data is implemented.

## Running available components

From the reconstruction clone:

```sh
cargo test --locked --all-targets --all-features
node --test scripts/tests/input-opposites.test.cjs
python3 -m unittest discover -s scripts/tests -p 'test_boot_server.py'
```

To serve the original archive **without modifying it**:

```sh
python3 web/serve.py --archive ../vertix-archive --port 8000 --socket-port 8001
```

That command runs the archival **HTTP bootstrap only**. The target Socket.IO
port 8001 is not started by it. To run the **experimental networked** PvP
compatibility server on a single loopback origin, instead use:

```sh
python3 -m web.pvp_server --archive ../vertix-archive --port 8000
```

The experimental Python backend and the separately-tested Rust reference
simulation currently have different code paths. They are **not production
complete** and must be consolidated and validated against two real clients.
The archival files must exist locally, including LFS content referenced by
the manifest. The shell is served over HTTP because the original client
hardcodes an HTTP socket URL.

## Acceptance gates remaining (NOT PASSED)

1. **Implement and verify on authentic clients** the provisional Engine.IO 3 polling and Socket.IO 1.x connection with
   `welcome` (2 args), `gotit` (4), `gameSetup` (JSON string + 2 args),
   `yourRoom`, positional update and `ping1`/`pong1` contracts.
2. Connect both client sessions to independent authoritative player
   identities, class selection and correct weapon objects in `gameSetup`.
3. Use appropriate map fixtures/PNG sources. Implement wall collision and
   projectile simulation (at minimum the chosen provisional hypothesis).
4. Serialize each `ReloadCompletion` as targeted event `r` with exactly
   one positional slot number. Reconcile ammo on both server and client.
5. Emit damage `1`, kill `3`, shot `2`, `rsd`, `upd`, `lb`
   in authentic 2016-08-06 shapes; validate with live browser clients.
6. Run two **authentic** independently controlled browser sessions:
   distinct classes/loadouts; weapon switches, firing, ammo depletion,
   reloads including weapon-switch-during-reload; PvP damage/kill/respawn;
   opponent state replicates; round completion.
7. Inject W + S and A + D on actual canvas with Playwright. Prove 0 velocity
   while both are held, and continuous original-direction velocity after
   releasing just the opposite key, both with and without input reconciliation.
8. Test timeout/disconnect, multiple players, malformed input, reload spam,
   tick/frame rate, map walls, jumping and latency. Preserve traces/screenshots.
9. Only after gates pass claim **full PvP working build**.

The research two-client probe exercises two authentic clients against a separate
synthetic research harness, not this new game's authoritative server. The branch's
new `test_pvp_server.py` exercises two **synthetic Engine.IO browser-equivalent
HTTP clients**, server side join/snapshot/damage/death and reload. It does **not**
exercise two authentic browser renderers, wall/projectile collisions, full game
round completion, or real network latency. Do not equate the passing synthetic
protocol tests with full PvP completion.

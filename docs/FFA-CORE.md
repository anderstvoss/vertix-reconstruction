# Deterministic FFA core — NEW reconstruction assumptions

Initial Rust **headless state engine** for [reconstruction #14](https://github.com/anderstvoss/vertix-reconstruction/issues/14). It is a tested-by-code simulation target, **not yet a networked game**, and no numbers in `Rules::synthetic_ffa()` are authenticated original-server balance.

## First-party / research constraints

The preserved 2016-08-06 Vertix client (source SHA-256 `cbab5cd590ff0d3a9d01a60eba835cd885b715988d0d87a7d287321399df2f09`) sends movement input event `"4"` with `hdt,vdt,ts,isn,s`, and client-side reconciliation expects latest processed `isn` in the `rsd` row. Its projectile animation locally uses its own weapon fields, but the original **server** governed hit rules, player health and score. Client event `"1"` has six positional fire arguments in this era. These are only **wire and client-code** facts; see research `analyses/STRICT-GAMEPLAY-PROFILE.md`.

## What is implemented

- `src/simulation.rs`: a deterministic independent `Match` with `BTreeMap` player ordering, monotonically increasing integer server ticks, unique player IDs, sequence-numbered client input, simple normalized movement, world boundaries, directional projectiles, collision-radius hits, HP, kills, respawns, ammo/cooldown/reload, scoreboard and winner threshold.
- `EvidenceStatus` names `Recovered/Provisional/New/Unresolved`; `Rules::synthetic_ffa()` is **entirely New**. `Unresolved` is rejected for executable rules rather than silently defaulted.
- Rejects invalid rules, duplicate players, input `isn` replays, multiple movement submissions per tick, invalid input direction/time values, illegal repeated shooting, dead-player shots and out-of-bounds spawn requests.
- Deterministic `Vec<Event>` log records each state transition; tests include damage/kill/respawn/round-end and repeated-script equality. No wall collision, sound, client integration or historical map loader is claimed.

**Deliberate design decisions (NEW):** fixed 10 ms server tick, fixed movement per accepted input, circular hit detection without line-of-sight walls, synthetic world bounds, synthetic bullet speed/HP/ammo/score thresholds, and fixed respawn locations. These are explicitly swappable; they were not discovered in old Vertix server binaries.

## Follow-up before playable client

1. Replace `Match::input`'s one-input-per-tick policy with a reviewed queue/input-accounting model that can accommodate real original-client frame rate and jitter without enabling timestamp cheating.
2. Add map collision `MapSource` interface and projectile line/swept collision; never silently claim candidate KRP maps are original. Original map authenticity is separately tracked by research #45.
3. Adapt events to **2016-specific positional** `rsd`, `upd`, `"1"` damage, `"3"` kill, `"7"` round end and `"8"` countdown. The simulation's Rust events are not those wire payloads.
4. Add Socket.IO poll sessions, archived-client page/assets, browser trace harness and two-player full round acceptance (#15).

## Test execution

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

The repository is public and has active GitHub Actions Rust build/test, formatting, lint and security jobs. The latest PR checks must pass before integration. A green unit-test run still does **not** establish historical browser interoperability or a playable server.

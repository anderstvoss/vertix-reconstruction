# Balance presets

Each file is one complete set of class and weapon numbers, chosen with
`balance` in `config/server.toml` (default `best`). `index.json` lists them.

- `best`: our inferred best overall. Late-game research evidence, with
  KRP's values where later patch notes make the evidence stale, and KRP for
  everything the research lacks (including all movement numbers).
- `krp`: KrunkerRevival's `loadouts.ts`, unchanged.
- `v0.20` to `v3.81`: one sheet per game version. Classes not yet in the
  game are marked `"available": false`.

Every value carries its `basis` (`research`, `backfill`, `krp` or
`inferred`) and `source`. `vertix-server --explain-rules` lists the values
of the selected preset.

These files are copied unchanged from the research repository's
`presets/balance/` (built by its `tools/build_balance_presets.py`, research
commit a3c71e7). Fix numbers there and copy them again; do not edit here.
Any class or weapon field a preset does not set comes from KRP
(`data/krp/loadouts.json`).

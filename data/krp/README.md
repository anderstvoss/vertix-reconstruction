# KrunkerRevival data

Game data from [KrunkerRevivalProject/vertix](https://github.com/KrunkerRevivalProject/vertix)
(KRP), the baseline engine this reconstruction ports. KRP is credited as
the source of these files and of the game logic in `src/game/`, which
follows KRP's `server/` and `core/src/logic/` code.

- `loadouts.json`: classes and weapons (`core/src/loadouts.ts`).
- `gamemodes.json`: the nine modes, in KRP's order (`core/src/gamemodes.ts`).
- `skins.json`: hats, shirts and camos (`core/src/skins.ts`).
- `sprays.json`: sprays (`core/src/sprays.ts`).

Each file records the KRP commit it came from. Regenerate with:

```bash
python3 scripts/import_krp.py PATH/TO/KRP-CHECKOUT
```

Class and weapon numbers are overridden by the selected balance preset
(`data/balance/`).

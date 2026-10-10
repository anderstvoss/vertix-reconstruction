# Cosmetics and mod packs

Hats, shirts, camos, sprays and community mod packs, restored from the
archive the same way maps are: the server reads each file from a local
vertix-archive clone at start-up and checks its SHA-256. No original file is
committed. `data/content/` only records where each file lives and its hash
(`scripts/import_content.py` writes it from the archive's asset database).

Recovered files are served ahead of the client build, so they replace
KRP's copies; whatever the archive lacks still comes from KRP's client
build (`client/dist`). Item names, descriptions and drop chances are KRP's
(`data/krp/skins.json`, `sprays.json`): the original server sent them and
no copy of that list was captured. Every item is unlocked (no accounts).

## What is restored

| Family | From the archive (first-party) | Still KRP's copy (fan repositories only) |
| --- | --- | --- |
| Hats | 1 to 116 and the hover image (`display.png`): Aug-2016 Android APK | 117 to 158 |
| Camos | 1 to 130 (there is no camo 88): Android APK | 131 to 133 |
| Sprays | 1 to 43 at their 2016 sizes (25 to 80 px): Android APK, Wayback | 44 to 83 (30 px) |
| Shirts | The hover image (`display.png`): Wayback 2016-12-20 | 1 to 80 (all shirt art) |
| Mod packs | The 2016 `res.zip` as pack `original-2016`, and 21 community packs: 2 from Wayback captures of their Dropbox originals (Sonic, Nuclear Throne), 19 from a fan repository's copies in the archive | none (KRP ships no packs) |

The research behind this is `assets-research/` in vertix-research: every
fan-repository copy of the 2016 art matches the APK byte for byte, and the
later art (hats 117+, camos 131+, sprays 44+, all shirts) has no
first-party copy anywhere yet (OPEN-GAPS).

**Versions.** Spray 10 has two first-party versions: the 25 px original
(served until at least 2019-10-06) and a 30 px re-size captured in 2020.
`[content] date` picks, for each file, the version that was live on that
day: `2017-07-01` (v3.8, the version KRP's client follows) by default.
A later date serves the re-size.

**Sprays** are served at both `/images/sprays/<id>.png` (the original path)
and `/assets/sprays/<id>.png` (where KRP's client loads them).

## Mod packs

A mod pack is a `vertixmod.zip` of replacement sprites, sounds and style
scripts. The stock game shipped silent, so mod packs are also the only way
to hear sounds.

- `/mods/` lists the packs with their keys, sprite and sound counts and
  where each copy came from; `/mods/index.json` is the same list as JSON.
- The game's MODS tab lists every pack the server offers, plus "No mods",
  which puts back the client's own art, menu title and classes and turns
  pack sounds off. A pack's key (for example `vertigo-mod`) can also be
  typed in and loaded with LOAD; the old Dropbox keys of the two Wayback
  packs work too. A path such as `/mods/vertigo-mod/vertixmod.zip` also
  works, and full URLs load as before.
- KRP's client sent keys to Dropbox (the links are dead) and could not load
  a path from its own server. `scripts/client-patches/apply.mjs` patches
  that in the pinned client source when `scripts/build-client.sh` builds
  it, replaces KRP's single Sonic button with the server's pack list and
  "No mods", and points the tab's "Get more mods here" link at `/mods/`.

**`original-2016`** is the game's own art as a pack: the 2016-08-04
`res.zip` from the Android APK (the one the 2016 client path serves), read
from the archive. The Rust client takes its 2016 floor tiles
(`sprites/ground1-3.png`) from it when the server offers it.

19 of the packs exist only inside a git bundle in the archive, which the
server cannot read directly. Unpack them once (into `content/mods/`,
git-ignored; each pack is checked against its hash):

```bash
python3 scripts/extract_mods.py --archive PATH/TO/vertix-archive
```

The server lists packs it cannot find as not unpacked in its start-up log
and serves the rest.

The packs are community works by their authors, kept for preservation.
They are served from your archive copy only, never committed.

## Settings

`[content]` in `config/server.toml`:

- `cosmetics = false` serves KRP's copies of every cosmetic instead.
- `date` picks among first-party versions, as above.
- `mods = false` turns off `/mods/`.
- `mods_dir` is where `extract_mods.py` unpacked the bundled packs.

## Still missing

- **Later cosmetic art** (hats 117 to 158, camos 131 to 133, sprays 44 to
  83, every shirt): no first-party copy. A later Android build, a Wayback
  or Common Crawl capture of `vertix.io/images/...`, or a dated mod pack
  carrying them would close this.
- **Item names and drop chances**: KRP's list, not recovered. The wiki's
  February 2019 spray screenshots name 63 sprays (research `a04`), which
  could check KRP's spray names.
- **Mod packs**: ten more Dropbox packs are known but were never captured;
  the official mod template (`vertixmod.zip`, Dropbox, deleted) is lost
  apart from the script files copied into community packs. KRP's
  commented-out mod buttons (Pacman, three Undertale packs, Black and
  White, Mario, Minecraft) name packs the archive does not hold; Polish
  Minecraft Mod may or may not be the Minecraft one.
- **Custom sprays** (a player's own image, v3.x) and the **hat loot**
  drops are not restored; with everything unlocked, loot has no purpose.

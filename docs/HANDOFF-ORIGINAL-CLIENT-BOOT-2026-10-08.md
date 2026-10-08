# Next agent: take the archived original 2016 client from menu to one playable guest session

**State:** this repository's [map/test PR #12](https://github.com/anderstvoss/vertix-reconstruction/pull/12) provides **24 source-locked candidate map conversions** and **43 map×mode tests**. The private archive [PR #9](https://github.com/anderstvoss/vertix-archive/pull/9) preserves the actual converted JSON files, original app.js and a browser-main-menu smoke test. **There is no confirmed gameplay server or full gameplay boot yet.**

**Canonical cross-repository execution guide:** [research agent handoff](https://github.com/anderstvoss/vertix-research/blob/research/architecture-gaps-source-triage-20261008/analyses/AGENT-HANDOFF-ARCHITECTURE-GAPS-2026-10-08.md). Read it and existing [milestone #2](https://github.com/anderstvoss/vertix-reconstruction/issues/2), [#3](https://github.com/anderstvoss/vertix-reconstruction/issues/3), [#5](https://github.com/anderstvoss/vertix-reconstruction/issues/5), [#6](https://github.com/anderstvoss/vertix-reconstruction/issues/6).

## Verified artifacts

- [Map converter](../scripts/vertix_maps/png_to_gen_data.py) creates `{width,height,data:{data:[RGBA...]}}`, not directly serialized `Uint8ClampedArray`.
- [Candidate source locks](../scripts/vertix_maps/krp_png_source_lock.csv) and [KRP-mode hypothesis list](../scripts/vertix_maps/krp_mode_hypotheses.csv), each for 24 numbered PNGs. The map↔mode mapping is a **third-party reconstruction hypothesis**.
- [Matrix runner](../scripts/vertix_maps/run_candidate_matrix.py) regenerates 43 synthetic `mapData` fixtures and produces a JSON report with `original_cases_executed`. It **must** read 43 when `--original-source` is provided; it reads **0** when source is omitted.
- [Node original-function probe](../scripts/vertix_maps/probe_original_setupmap.cjs) executes actual 2016 `setupMap`/`canPlaceFlag` source under a fingerprint guard in a VM, not full browser initialization.
- [CI](https://github.com/anderstvoss/vertix-reconstruction/actions/runs/37848637656): 24/24 PNG conversions, 43/43 generated structural fixtures and four regression tests. [Archive original-source CI](https://github.com/anderstvoss/vertix-archive/actions/runs/37849302994): **43/43 full-source function executions**.
- [Archive original-client Chrome smoke](https://github.com/anderstvoss/vertix-archive/actions/runs/37850006821): byte-verified `app.js` reaches visible main menu and tries `/socket.io/?EIO=3&transport=polling`, **not** an active game room.

## How to reproduce

Use sibling local clones, checkout this PR branch or its merged commit, and pin the KRP checkout to `2d35f665917c86691936d49079424521da6d80b6`.

```bash
python3 -m unittest scripts/tests/test_vertix_map_fixture.py -v

python3 scripts/vertix_maps/run_candidate_matrix.py \
  ../vertix-krp/server/maps /tmp/vertix-map-fixtures \
  --original-source ../vertix-archive/research/analysis/app/beautified/app-20160806061006-FEMVAY3YIDEPBGEXH32YHQNXPFQDB4N3.js

# Inspect /tmp/vertix-map-fixtures/manifest.json; verify:
# verified_png_count == 24
# fixture_cases == 43
# original_source_supplied == true
# original_setupMap_test == "PASS" for all 43 cases
```

Only read archived original assets at runtime from a verified local `VERTIX_ARCHIVE` checkout; **never commit them** to this repository. Original August 6 `app.js` SHA-256: `cbab5cd590ff0d3a9d01a60eba835cd885b715988d0d87a7d287321399df2f09`.

## Implement next (separate reviewable PRs)

### PR 1. EIO3 / Socket.IO 1.4.5 handshake and static asset host

- Serve byte-verified first-party client, script dependencies, CSS, `res.zip` and workers without changing source. The archive already has the needed copies. Isolate the page from external hosts.
- Supply a locally configured `/getIP` returning `{ip,port}` with the real socket listener. Do not serve old `/getIP` captures. **This repository's security hooks forbid hard-coded loopback/private-IP literals**; use runtime settings or put literal loopback test service in the private archive.
- Implement Engine.IO **3** polling and Socket.IO **1.x** namespace/event framing. Test handshake open packet, framed multi-packet polling including Unicode length, ping/pong, reconnect/disconnect, invalid SID, and events with multiple **positional arguments**.
- **Gate:** actual archived client connects to a genuine local Socket.IO session in Chrome; machine-readable encoded-packet trace, client/network errors and no outbound requests. The existing screenshot of the main menu is a *starting point*, not completion.

### PR 2. Guest bootstrap + one active map

- Implement `yourRoom(room,key)`, `welcome(player, flag)` → client `gotit(...)` → `gameSetup(serializedJSON, flag, scalar)` as recovered from original handler. Check exact meanings/positional argument types in [research event contracts](https://github.com/anderstvoss/vertix-research/blob/research/architecture-gaps-source-triage-20261008/analyses/ARCHITECTURE-GAPS-RESEARCH-EXECUTED.md) before filling values.
- Populate `gameSetup` fields `mapData`, `maxScreenWidth`, `maxScreenHeight`, `tileScale`, `usersInRoom`, `viewMult`, `you`. Derive map RGB bytes from an [archive-derived `map-N.genData.json`](https://github.com/anderstvoss/vertix-archive/tree/research/krp-map-converted-evidence-20261008/vertix-preservation/derived/maps/krp-2026-candidates); **the original 2016 handler needs `genData.data.data`**. Populate required gameMode, clutter/pickups, map dimensions and initial player data, **without claiming KRP choices are historical**.
- Weapon configuration must pass the [33-field client schema](https://github.com/anderstvoss/vertix-research/blob/research/architecture-gaps-source-triage-20261008/analyses/WEAPON-BOOT-FIELD-MATRIX.csv), keep per-player mutable data independent and explicitly label synthetic values. Keep guest-only; do not mimic original account credentials.
- **Gate:** original unchanged browser receives `gameSetup`, calls `setupMap`, produces visible world/collision layout, and has no unexplained JS exceptions. Record an interactive-world screenshot and source hashes; this gate was **not** reached by prior work.

### PR 3. Real two-browser movement and shooting

- Treat `"4"` movement `delta` as **optional/usually absent**. The normal Socket.IO 1.4.5 serialized event fields are `hdt,vdt,ts,isn,s`; local replay mutation `delta` occurs after `emit`. Do not write `pos += speed * data.delta` unguarded.
- Derive your own bounded elapsed server time policy, mark it a reconstruction assumption; test ACK `isn`, prediction replay, remote `rsd` flat packet shape and fan-out to **other connected players**, movement while one peer stands still.
- Fix original inbound damage `"1"` keys (`amount,bi,h`), round end `"7"` four positional args, countdown `"8"` one scalar, independent mutable weapons and KRP's mismatched `/api/getIP`.
- **Gate:** two simultaneous authentic browser clients successfully spawn, see remote movement and complete a guest-only FFA round (shoot, damage, death, respawn, score, round end) with reproducible trace. Do not claim historical server physics or weapon balance.

## Stop conditions and evidence hygiene

- All KRP PNGs are **unverified third-party candidate maps**. Embedded PNG 2016 timestamps and passed original bitmap parsing are **not** original server provenance; [research issue #45](https://github.com/anderstvoss/vertix-research/issues/45) pursues source attribution.
- Client `FRAME_STEP=1000/60` is not evidence of a 60Hz server tick. Weapon fields and local UI delays are not the original server's authoritative timings.
- Static source/probe unit tests do not satisfy real client boot or multi-client runtime acceptance.
- Do not add altered/copied original client bytes, hard-coded real production IPs, account credentials or unsourced authentic-sounding constants.
- Finish each PR with: precise source commit, command/CI run, screenshot/packet trace, assertions tested, synthetic parameter labels, open failed gates and links to research/archive issues.

**For historical unknowns:** [timing research #16](https://github.com/anderstvoss/vertix-research/issues/16), [combat #19](https://github.com/anderstvoss/vertix-research/issues/19), [modes #20](https://github.com/anderstvoss/vertix-research/issues/20), [persistence #21](https://github.com/anderstvoss/vertix-research/issues/21). New implementation decisions belong in a clearly typed assumptions overlay.

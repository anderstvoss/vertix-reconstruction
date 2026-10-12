# Acceptance: two unmodified 2016 clients complete an FFA round

Machine-checkable fixture: [`tests/fixtures/ffa-original-client-acceptance.json`](../tests/fixtures/ffa-original-client-acceptance.json).

This is the exact acceptance contract for [#15](https://github.com/anderstvoss/vertix-reconstruction/issues/15) and umbrella milestone #6. Its `status=NOT_EXECUTED` is intentional. Do not mark the milestone complete because the Rust domain simulation, synthetic Python harness or this acceptance-document regression test passes.

## Four sequential stages

1. **Connect and join:** Both independent browser contexts load the verified archived assets, negotiate Engine.IO 3 and receive a private `welcome` (2 positional arguments); both emit `gotit` (4), `create` (0 or 1 depending on join path), and `respawn` (0).
2. **Set up the same world and move:** The Rust server sends `gameSetup` (3, first argument is a JSON string) to each, with individual local player state. Both clients emit input `"4"` (1 object with `hdt,vdt,ts,isn,s`), and server `rsd` (1 flat length-prefixed array) acknowledges processed `isn` while updating the peer.
3. **Combat and respawn:** An unmodified client emits six positional fire values via `"1"`. The server alone evaluates **NEW provisional** hit rules and sends the matching `"1"` damage (one object), `"3"` death/kill (one object), `upd` score (one object), and subsequent respawn state.
4. **Round end:** The server reaches a **NEW provisional** score threshold, sends `"7"` with four positional arguments and `"8"` with one scalar. The original clients display distinct victory/defeat and countdown states.

The fixture also mandates rejection/safe handling of duplicate inputs, invalid repeated firing, disconnect, private-state leaks and malformed messages. Packet traces and observable browser state/screenshot evidence from both sessions are required.

## Evidence and provenance

- Original-client build: 2016-08-06 `app.js`, SHA-256 `cbab5cd590ff0d3a9d01a60eba835cd885b715988d0d87a7d287321399df2f09`.
- Archive commit: `a059176db6671db6f35c1a3e2457495c08c846ac`.
- Source of client event directions/arity: `anderstvoss/vertix-research/analyses/out/contracts/20160806061006.json` and `analyses/RECONSTRUCTION-BASE.md`.
- **NOT RECOVERED:** the original server executable/source; historical map generator, damage, tick rate, spawn selection, hit and scoring policies. These must live in replaceable `NEW` or explicitly `PROVISIONAL` server configuration.

`scripts/tests/test_original_ffa_acceptance.py` runs with standard Python unittest to enforce the static contract and `NOT_EXECUTED` marker. It does not launch Chromium. The actual browser tests require the private original archive as described in [research's existing two-client probe](https://github.com/anderstvoss/vertix-research/blob/main/runtime/TWO-CLIENT-VALIDATION.md), then a new driver that targets the Rust server, rather than only the synthetic injector.

**Done means:** a reproducible automated run produces a per-session raw transport trace, screenshots, client state assertions, actual Rust domain state events and a checksum of the explicitly provisional rules. It must **not** modify or redistribute the original client in this public repository.

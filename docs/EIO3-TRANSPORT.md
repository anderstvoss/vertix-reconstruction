# 2016 client transport: implementation boundary

First implementation step for [#13](https://github.com/anderstvoss/vertix-reconstruction/issues/13). This is a **new Rust codec**, not recovered original game code and not yet a network server.

## Grounded wire facts

The source-pinned research client contract (2016-08-06 SHA-256 `cbab5cd590ff0d3a9d01a60eba835cd885b715988d0d87a7d287321399df2f09`) and preserved 2016-01-18 Engine.IO polling handshake show:

- Engine.IO protocol 3 text polling packets are prefixed by **UTF-16 code unit lengths**; the archived XHR2 binary payload frames use `0x00`, numeric decimal length digits, `0xff`, then UTF-8 text bytes. A recorded 97-byte handshake begins `00 09 07 ff`.
- Engine.IO ping/pong (`2` / `3`) and Socket.IO default-namespace connect/event (`40` / `42[...]`) are distinct wire envelopes. `42["8",5]` includes one **positional** event argument, not a payload object; `42["7",3,[],[],false]` includes four.
- Original-client browser tests previously established that long-polling alone is workable. The production page and socket were cross-origin, requiring credential-aware CORS. `GET /getIP` is dynamic. The client requires a HEAD response for `res.zip` before the asset ZIP GET.

## What this PR adds

`src/transport.rs` is a dependency-free bounded encoder/decoder for **text-only Engine.IO packets** in both framing variants, preserving non-ASCII UTF-16/UTF-8 distinctions. It identifies a minimal set of Socket.IO envelopes but **does not JSON-parse their contents**; `EventJsonArray` is a raw, untrusted payload for a separate reviewed parser/typed adapter.

Size bounds (1 MiB frame; 4 MiB payload; 256 packets) are **NEW defensive design limits**, not recovered historical server parameters. Non-text Socket.IO attachments, namespace variants, HTTP/Socket.IO liveness, per-session queues and the browser-facing server remain unimplemented.

Tests include archived framing examples, multiple packets and unicode, malformed lengths/surrogates, hostile oversized payloads, and positional event envelope preservation.

## Next integration gates

1. Implement strict JSON parsing for the `42` event array with numeric event names and positional values; reject malformed or unknown data without panics.
2. Implement HTTP polling sessions and heartbeat with bounded queues, HEAD assets, dynamic `/getIP` and privacy-safe localhost binding. Reference [research runtime probes](https://github.com/anderstvoss/vertix-research/blob/main/runtime/TWO-CLIENT-VALIDATION.md); **no archived assets in the public repository**.
3. Run the two real-browser client probe against this Rust adapter and the pinned private archive, recording the original-client runtime trace.
4. Connect decoded `4` input / `1` shot / `r` reload to the deterministic authoritative FFA core introduced separately.

The code is intentionally not advertised as a bootable Vertix server until those interfaces are implemented. Current CI in this repository is gated while private; test run status must be reported separately rather than assumed.

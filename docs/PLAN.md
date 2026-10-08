# Plan

Milestones, in order. Each one ends with a reproducible command and a trace,
and records what was tested and what was not.

## 1. The original client boots against a local server

- **Transport.** Engine.IO 3 / Socket.IO 1.x over long-polling, the protocol
  the 2016 client speaks: length-prefixed polling payloads, client-sent ping
  and server pong, Socket.IO events with positional arguments. Tested against
  the archived Socket.IO client, not only against our own test client.
  WebSocket upgrade can come later.
- **Page.** Serve the byte-exact 2016-08-06 `app.js` with the August 2016
  APK assets, read from a local archive clone and hash-checked. The archive
  has no same-day `index.html` for this build; the page shell comes from the
  nearest capture or the APK, and that choice is recorded.
- **Routing.** The client asks `/getIP` for a server address and connects to
  it. The local server answers with its own address. No test may contact the
  original servers; outbound requests are blocked while testing.
- **Done when:** the unmodified client reaches a rendered game state in a
  real browser, with a startup trace and screenshots.

## 2. Event contracts

Each of the 39 client-to-server and 51 server-to-client events, with
argument count, order and types taken from the client's own handlers and
tested by sending them to the running client. Known example: the end-of-round
event `7` takes four positional arguments, and the countdown event `8` takes
one scalar (it is shown as `<value>: UNTIL NEXT ROUND`). The static
extraction lives in the research repository; the runtime tests live here.

## 3. Movement, collision and configuration

Measure client-side movement, collision, camera and interpolation with
controlled inputs. Weapon and class numbers come from the research
repository's dated evidence for the target build; anything not known for
that date goes in an explicit assumptions file, never inline in game logic.

## 4. One complete free-for-all round

Join, spawn, move, shoot, damage, kill, respawn, score and round end, played
by two unmodified browser clients. Hit rules, spawn selection, score limit
and anti-cheat are reconstruction choices and are labelled as such.

## Notes

- The template's `block-local-network-targets` hook rejects loopback
  addresses anywhere in the tree. When the server needs a default bind
  address, keep it in one config file and add a narrow `exclude` for that
  file only.
- Deviations from the original behaviour (security fixes, modern netcode)
  are recorded in a deviations list as they are made.

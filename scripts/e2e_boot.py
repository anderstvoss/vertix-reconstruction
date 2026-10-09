#!/usr/bin/env python3
"""Milestone 1 check: the unmodified 2016 client boots and plays against our server.

Drives real Chromium (Playwright) against a running `vertix-server`:

  1. loads the page and waits for the menu,
  2. enters a name and presses Play, then waits until the client's own
     state says the game started (`gameStart`, a player index, the map),
  3. holds a movement key and checks the client moved and the server agreed,
  4. joins a second browser and checks each client sees the other.

Every request to anything but the server is blocked and recorded, so a
pass also proves the client never contacted the original servers or a
third party. Screenshots, browser console output and a JSON report go to
--out (git-ignored by default).

    python3 scripts/e2e_boot.py --url http://<host>:<port>/ [--chromium PATH] [--out out/e2e]

Exit status 0 = every check passed.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from urllib.parse import urlsplit

try:
    from playwright.sync_api import sync_playwright
except ImportError:  # pragma: no cover - documented prerequisite
    sys.exit("needs Playwright: python3 -m pip install playwright")


class Run:
    def __init__(self, base: str, out: Path):
        self.base = base
        self.origin = urlsplit(base)._replace(path="", query="", fragment="").geturl()
        self.out = out
        self.checks: list[dict] = []
        self.blocked: list[str] = []
        self.requests: list[str] = []
        self.console: list[str] = []
        self.page_errors: list[str] = []

    def check(self, name: str, ok: bool, detail: object = None) -> bool:
        self.checks.append({"check": name, "ok": bool(ok), "detail": detail})
        print("%s %s%s" % ("PASS" if ok else "FAIL", name, "" if detail is None else "  %s" % (detail,)))
        return ok

    def attach(self, page, label: str) -> None:
        def route(r):
            url = r.request.url
            if url.startswith(self.origin + "/") or url.startswith("blob:") or url.startswith("data:"):
                self.requests.append(urlsplit(url).path)
                r.continue_()
            else:
                self.blocked.append(url)
                r.abort()

        page.route("**/*", route)
        page.on("console", lambda m: self.console.append("[%s] %s: %s" % (label, m.type, m.text)))
        page.on("pageerror", lambda e: self.page_errors.append("[%s] %s" % (label, e)))

    def state(self, page) -> dict:
        return page.evaluate(
            """() => ({
                gameStart: window.gameStart, inMainMenu: window.inMainMenu,
                index: window.player && window.player.index, dead: window.player && window.player.dead,
                x: window.player && window.player.x, y: window.player && window.player.y,
                tiles: window.gameMap && window.gameMap.tiles ? window.gameMap.tiles.length : 0,
                players: (window.gameObjects || []).filter(o => o.type == 'player').length,
                othersOnScreen: (window.gameObjects || []).filter(o => o.type == 'player'
                    && o.index !== window.player.index && o.onScreen).length,
                weapons: window.player && window.player.weapons ? window.player.weapons.length : 0,
                ping: (document.getElementById('pingText') || {}).innerHTML,
                loadText: (document.getElementById('loadText') || {}).innerHTML,
            })"""
        )

    def wait_for(self, page, cond: str, timeout: float = 15.0) -> bool:
        deadline = time.time() + timeout
        while time.time() < deadline:
            if page.evaluate("() => { try { return !!(%s); } catch (e) { return false; } }" % cond):
                return True
            time.sleep(0.1)
        return False

    def join(self, page, name: str, shot: str) -> dict:
        page.goto(self.base, wait_until="load")
        self.check("%s: menu is ready (socket connected, Play bound)" % name,
                   self.wait_for(page, "document.getElementById('startButton').onclick != null"))
        page.screenshot(path=str(self.out / ("%s-menu.png" % shot)))
        page.fill("#playerNameInput", name)
        page.click("#startButton")
        started = self.wait_for(page, "window.gameStart === true && window.player.index !== undefined"
                                      " && window.gameMap && window.gameMap.tiles.length > 0")
        st = self.state(page)
        self.check("%s: game started from the server's gameSetup" % name, started, st)
        time.sleep(1.5)
        page.screenshot(path=str(self.out / ("%s-ingame.png" % shot)))
        return st


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--url", required=True, help="the server's page URL")
    ap.add_argument("--chromium", help="Chromium executable, if Playwright's own is not installed")
    ap.add_argument("--out", type=Path, default=Path("out/e2e"))
    ap.add_argument("--headed", action="store_true")
    args = ap.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    run = Run(args.url, args.out)

    with sync_playwright() as pw:
        launch = {"headless": not args.headed}
        if args.chromium:
            launch["executable_path"] = args.chromium
        browser = pw.chromium.launch(**launch)
        ctx_a = browser.new_context(viewport={"width": 1280, "height": 720})
        page_a = ctx_a.new_page()
        run.attach(page_a, "A")
        before = run.join(page_a, "Alpha", "a")

        # Hold "D" (the client's default right key) and let it predict.
        page_a.click("#cvs", position={"x": 900, "y": 360})
        page_a.keyboard.down("d")
        time.sleep(1.2)
        page_a.keyboard.up("d")
        time.sleep(0.6)
        after = run.state(page_a)
        moved = (after["x"] or 0) - (before["x"] or 0)
        run.check("A: holding D moves the player right", moved > 100, {"dx": moved})
        page_a.screenshot(path=str(args.out / "a-moved.png"))

        ctx_b = browser.new_context(viewport={"width": 1280, "height": 720})
        page_b = ctx_b.new_page()
        run.attach(page_b, "B")
        run.join(page_b, "Bravo", "b")
        sees = run.wait_for(page_a, "window.gameObjects.filter(o => o.type == 'player').length == 2")
        run.check("A knows about B (add)", sees, run.state(page_a))
        run.check("B knows about A (usersInRoom)",
                  run.wait_for(page_b, "window.gameObjects.filter(o => o.type == 'player').length == 2"),
                  run.state(page_b))
        time.sleep(2.5)
        run.check("ping1/pong1 round trip shows a ping", "PING" in (run.state(page_a)["ping"] or ""),
                  run.state(page_a)["ping"])
        page_b.screenshot(path=str(args.out / "b-sees-a.png"))

        # Leave the way Socket.IO does on a clean disconnect. A tab that just
        # vanishes is dropped after pingInterval + pingTimeout (85 s), as in
        # the original handshake's timing.
        page_b.evaluate("() => window.socket.disconnect()")
        gone = run.wait_for(page_a, "window.gameObjects.filter(o => o.type == 'player').length == 1")
        run.check("A drops B after B leaves (rem)", gone, run.state(page_a))
        ctx_b.close()
        browser.close()

    run.check("no request left the server (blocked: %d)" % len(run.blocked), not run.blocked, run.blocked[:20])
    run.check("no uncaught page errors", not run.page_errors, run.page_errors[:20])
    report = {
        "url": args.url,
        "checks": run.checks,
        "blocked_requests": run.blocked,
        "requested_paths": sorted(set(run.requests)),
        "page_errors": run.page_errors,
        "console": run.console,
    }
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    failed = [c for c in run.checks if not c["ok"]]
    print("%d/%d checks passed; report in %s" % (len(run.checks) - len(failed), len(run.checks), args.out))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

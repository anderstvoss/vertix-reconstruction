#!/usr/bin/env python3
"""End-to-end smoke test: start vertix-server and play through it the way
KRP's client does, over long-polling and over WebSocket.

    cargo build && python3 scripts/e2e_smoke.py [--server target/debug/vertix-server]

Needs no archive and no client build: it starts the server with our own
text arena and a stand-in client directory. Standard library only.
"""

from __future__ import annotations

import argparse
import base64
import http.client
import json
import os
import re
import socket
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SEP = "\x1e"
CHECKS: list[tuple[str, bool]] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    CHECKS.append((name, ok))
    print(f"{'PASS' if ok else 'FAIL'}  {name}{'  ' + detail if detail and not ok else ''}")


def request(base: tuple[str, int], path: str, body: bytes | None = None) -> tuple[int, bytes]:
    conn = http.client.HTTPConnection(*base, timeout=40)
    try:
        headers = {"Content-Type": "text/plain;charset=UTF-8"} if body is not None else {}
        conn.request("POST" if body is not None else "GET", path, body=body, headers=headers)
        r = conn.getresponse()
        return r.status, r.read()
    finally:
        conn.close()


def events(packets: list[str], ns: str) -> list[list]:
    out = []
    for p in packets:
        prefix = f"42{ns},"
        if p.startswith(prefix):
            out.append(json.loads(p[len(prefix):]))
    return out


class Polling:
    def __init__(self, base: tuple[str, int]):
        self.base = base
        status, body = request(base, "/socket.io/?EIO=4&transport=polling")
        assert status == 200, status
        self.open = json.loads(body.decode()[1:])
        self.sid = self.open["sid"]

    def url(self) -> str:
        return f"/socket.io/?EIO=4&transport=polling&sid={self.sid}"

    def send(self, *packets: str) -> None:
        status, _ = request(self.base, self.url(), SEP.join(packets).encode())
        assert status == 200, status

    def poll(self) -> list[str]:
        status, body = request(self.base, self.url())
        assert status == 200, status
        return body.decode().split(SEP)

    def until(self, ns: str, name: str, limit: int = 20) -> tuple[list, list[list]]:
        seen: list[list] = []
        for _ in range(limit):
            for p in self.poll():
                if p == "2":
                    self.send("3")
                    continue
                for ev in events([p], ns):
                    seen.append(ev)
                    if ev[0] == name:
                        return ev, seen
        raise AssertionError(f"no {name} in {[e[0] for e in seen]}")


class WebSocket:
    """Just enough of RFC 6455 for text frames."""

    def __init__(self, host: str, port: int, path: str, extra: str = ""):
        self.sock = socket.create_connection((host, port), timeout=10)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(
            (
                f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n"
                f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n{extra}\r\n"
            ).encode()
        )
        head = b""
        while b"\r\n\r\n" not in head:
            head += self.sock.recv(1)
        self.status = int(head.split(b" ")[1])
        self.buf = b""

    def send(self, text: str) -> None:
        data = text.encode()
        mask = os.urandom(4)
        n = len(data)
        if n < 126:
            hdr = struct.pack("!BB", 0x81, 0x80 | n)
        else:
            hdr = struct.pack("!BBH", 0x81, 0x80 | 126, n)
        self.sock.sendall(hdr + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def _read(self, n: int) -> bytes:
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError("closed")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv(self) -> str | None:
        b0, b1 = self._read(2)
        n = b1 & 0x7F
        if n == 126:
            (n,) = struct.unpack("!H", self._read(2))
        elif n == 127:
            (n,) = struct.unpack("!Q", self._read(8))
        data = self._read(n)
        if b0 & 0x0F == 8:
            return None
        return data.decode()

    def until(self, ns: str, name: str, limit: int = 200) -> list:
        for _ in range(limit):
            p = self.recv()
            if p is None:
                break
            if p == "2":
                self.send("3")
            for ev in events([p], ns):
                if ev[0] == name:
                    return ev
        raise AssertionError(f"no {name}")


def gotit(name: str, cls: int) -> str:
    return "42/DEV0," + json.dumps(["gotit", {"name": name, "classIndex": cls}, False, 0, False])


def run(host: str, port: int) -> tuple[WebSocket, int]:
    base = (host, port)
    status, body = request(base, "/")
    check("serves the client directory", status == 200 and b"stand-in" in body)
    check("keeps requests inside it", request(base, "/..%2f..%2fCargo.toml")[0] == 404)
    status, body = request(base, "/api/getRooms")
    rooms = json.loads(body)
    check("lists the rooms", status == 200 and [r["n"] for r in rooms] == ["DEV0", "DEV1"], str(rooms))
    status, body = request(base, "/api/getIP?room=DEV1")
    ip = json.loads(body)
    check("getIP names the room and our own port", ip["room"] == "DEV1" and ip["port"] == str(port), str(ip))
    check("getIP falls back to the first room", json.loads(request(base, "/api/getIP?room=zzz")[1])["room"] == "DEV0")
    check("leaderboards are empty", json.loads(request(base, "/api/getLbs")[1])["rank"] == [])

    a = Polling(base)
    check("Engine.IO 4 handshake", a.open.get("upgrades") == ["websocket"] and a.open.get("pingInterval") == 25000)
    a.send("40/DEV0,")
    welcome, seen = a.until("/DEV0", "welcome")
    check("joins the room namespace", seen[0] == ["yourRoom", "DEV0"] and welcome[2] is True)
    me = welcome[1]["id"]
    a.send(gotit("Alpha", 0))
    setup, seen = a.until("/DEV0", "gameSetup")
    doc = json.loads(setup[1])
    check("spawns with a game setup", doc["you"]["name"] == "Alpha" and not doc["you"]["dead"])
    x0 = doc["you"]["x"]
    a.send("42/DEV0," + json.dumps(["4", {"hdt": 1, "vdt": 0, "delta": 16, "isn": 1, "s": 0}]))
    rsd, _ = a.until("/DEV0", "rsd")
    flat = rsd[1]
    mine = [flat[i : i + 6] for i in range(0, len(flat), 6) if flat[i + 1] == me][0]
    check("moves and echoes positions", mine[2] > x0 and mine[5] == 1, str(mine))

    ws = WebSocket(host, port, f"/socket.io/?EIO=4&transport=websocket&sid={a.sid}")
    check("WebSocket upgrade accepted", ws.status == 101)
    ws.send("2probe")
    check("upgrade probe answered", ws.recv() == "3probe")
    ws.send("5")
    ws.send("42/DEV0," + json.dumps(["cht", "hello there", "ALL"]))
    cht = ws.until("/DEV0", "cht")
    check("chat after the upgrade", cht[1] == [me, "hello there"], str(cht))

    b = WebSocket(host, port, "/socket.io/?EIO=4&transport=websocket")
    opened = json.loads(b.recv()[1:])
    check("WebSocket-only clients get an open packet", "sid" in opened)
    b.send("40/DEV0,")
    b.until("/DEV0", "welcome")
    b.send(gotit("Bravo", 2))
    add = ws.until("/DEV0", "add")
    other = json.loads(add[1])
    check("others see the new player", other["name"] == "Bravo" and other["classIndex"] == 2, str(other)[:200])
    b.send("41/DEV0,")
    rem = ws.until("/DEV0", "rem")
    check("leaving removes the player", rem[1] == other["index"])
    rooms = json.loads(request(base, "/api/getRooms")[1])
    check("room counts follow", rooms[0]["pl"] == 1, str(rooms))
    return ws, me


def admin(base: tuple[str, int], token: str, line: str, auth: bool = True) -> tuple[int, dict]:
    conn = http.client.HTTPConnection(*base, timeout=10)
    try:
        headers = {"Content-Type": "application/json"}
        if auth:
            headers["Authorization"] = f"Bearer {token}"
        conn.request("POST", "/api/cmd", body=json.dumps({"line": line}), headers=headers)
        r = conn.getresponse()
        body = r.read()
        return r.status, (json.loads(body) if r.status == 200 else {})
    finally:
        conn.close()


def run_admin(host: str, port: int, token: str, ws: WebSocket, me: int, stdin) -> None:
    """The admin port and the terminal console, seen from a player."""
    base = (host, port)
    status, body = request(base, "/")
    check("admin panel page is served", status == 200 and b"Server Admin" in body)
    check("admin API needs the token", admin(base, token, "status", auth=False)[0] == 401)
    check("admin API refuses a wrong token", admin(base, "nope" + token, "status")[0] == 401)
    status, r = admin(base, token, "status")
    check("admin status lists rooms", status == 200 and r["ok"] and len(r["data"]["rooms"]) == 2, str(r)[:200])
    status, r = admin(base, token, "players")
    check("admin sees the player", "Alpha" in r.get("text", ""), str(r))

    admin(base, token, "kill Alpha")
    kill = ws.until("/DEV0", "3")
    check("kill slays the player", kill[1]["gID"] == me and kill[1]["sS"] == 0, str(kill))
    admin(base, token, "win none")
    ws.until("/DEV0", "7")
    check("win ends the round", True)
    admin(base, token, "restart")
    welcome = ws.until("/DEV0", "welcome")
    check("restart sends players to the menu", welcome[2] is True)
    _, r = admin(base, token, "@DEV1 mode hp")
    check("mode changes another room", r.get("ok") and "hp" in r["text"], str(r))
    _, r = admin(base, token, "frobnicate")
    check("unknown commands are refused", r.get("ok") is False)

    bad = WebSocket(host, port, f"/ws?token={token}", "Origin: http://elsewhere.test\r\n")
    check("panel socket refuses other origins", bad.status == 403)
    pw = WebSocket(host, port, f"/ws?token={token}", f"Origin: http://{host}:{port}\r\n")
    check("panel socket opens", pw.status == 101)
    pw.send(json.dumps({"id": 7, "line": "rooms"}))
    reply = json.loads(pw.recv())
    check("panel socket answers commands", reply["id"] == 7 and reply["reply"]["ok"], str(reply)[:200])

    stdin.write("say hello from the terminal\n")
    stdin.flush()
    cht = ws.until("/DEV0", "cht")
    check("terminal commands reach players", cht[1] == [-1, "hello from the terminal"], str(cht))
    seen = None
    for _ in range(20):
        m = json.loads(pw.recv())
        if m["type"] == "log" and m["line"]["kind"] == "chat":
            seen = m["line"]
            break
    check("panel log shows chat", seen is not None and "hello from the terminal" in seen["text"], str(seen))

    admin(base, token, "kick Alpha Bye now")
    kick = ws.until("/DEV0", "kick")
    check("kick tells the client why", kick[1] == "Bye now", str(kick))
    rooms = json.loads(request((host, port - 2), "/api/getRooms")[1])
    check("kick frees the seat", rooms[0]["pl"] == 0, str(rooms))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    exe = "vertix-server.exe" if os.name == "nt" else "vertix-server"
    ap.add_argument("--server", default=str(ROOT / "target/debug" / exe))
    ap.add_argument("--port", type=int, default=18080)
    args = ap.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        client = Path(tmp) / "client"
        client.mkdir()
        (client / "index.html").write_text("<!doctype html><title>stand-in</title>stand-in client\n")
        config = (ROOT / "config/server.toml").read_text()
        # The committed config is the one place that names the bind address.
        host = re.search(r'^bind = "([^"]+)"', config, re.M).group(1)
        config = config.replace('sources = ["archive"]', 'sources = ["files"]')
        config = config.replace("port = 8082", f"port = {args.port + 2}")
        config = config.replace('client_dir = "client/dist"', f"client_dir = {json.dumps(str(client))}")
        start = config.index("rooms = [")
        end = config.index("]\n", config.index("pyro")) + 2
        config = (
            config[:start]
            + 'rooms = [{ name = "DEV0", mode = "ffa" }, { name = "DEV1", mode = "tdm" }]\n'
            + config[end:]
        )
        cfg = Path(tmp) / "server.toml"
        cfg.write_text(config)
        proc = subprocess.Popen(
            [args.server, "--config", str(cfg), "--port", str(args.port)],
            cwd=ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            assert proc.stderr is not None
            token = ""
            for line in proc.stderr:
                if "#token=" in line:
                    token = line.strip().split("#token=")[1]
                if "listening on" in line:
                    break
                if proc.poll() is not None:
                    break
            if proc.poll() is not None:
                print("server did not start", file=sys.stderr)
                return 1
            time.sleep(0.1)
            ws, me = run(host, args.port)
            run_admin(host, args.port + 2, token, ws, me, proc.stdin)
        finally:
            proc.terminate()
            proc.wait(timeout=10)
    failed = [n for n, ok in CHECKS if not ok]
    print(f"{len(CHECKS) - len(failed)}/{len(CHECKS)} checks passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

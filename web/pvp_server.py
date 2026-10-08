"""Experimental, loopback-only two-player EIO3 / Socket.IO 1.x FFA server.

Run: python3 -m web.pvp_server --archive ../vertix-archive --port 8000

This is a provisional Python compatibility prototype separate from the tested
Rust gameplay reference. The original server source is unavailable; synthetic
maps, combat, reload durations and scores are NOT historically recovered.
Original game bytes are always loaded and SHA-256 verified by web.serve.
"""
from __future__ import annotations

import argparse
import json
import math
import secrets
import threading
import time
from dataclasses import dataclass, field
from http.server import ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from web.serve import ArchiveAssets, Handler as AssetHandler

ROOM = "local-ffa"
MAX_PACKET_BYTES = 2_000_000
RELOADS_MS = [1500, 2200, 1200, 1000, 2000, 2000, 1200, 900, 2000, 1200]
MAGAZINES = [24, 6, 1, 8, 3, 1, 1, 6, 50, 30]
DAMAGE = [20, 50, 100, 17, 22, 110, 110, 12, 8, 10]
FIRE_RATES_MS = [143, 390, 1200, 78, 143, 1200, 1200, 120, 90, 70]
CLASS_WEAPONS = [(0, 5), (1, 5), (2, 7), (3,), (4, 5), (6,), (8,), (9,), (9,)]
MAP_SIDE = 24
TILE = 40


def event_frame(name: str, *args: object) -> str:
    return "42" + json.dumps([name, *args], separators=(",", ":"), ensure_ascii=False)


def encode_packets(packets: list[str]) -> bytes:
    # Original EIO3 client expects UTF-16 code-unit lengths, not byte lengths.
    return "".join(
        f"{len(packet.encode('utf-16-le')) // 2}:{packet}"
        for packet in packets
    ).encode("utf-8")


def decode_packets(body: bytes) -> list[str]:
    if len(body) > MAX_PACKET_BYTES:
        raise ValueError("oversized Engine.IO message")
    if body.startswith(b"\x00"):
        # EIO3 XHR2 binary framing (prefix 0 = UTF-8 text packet).
        packets, i = [], 0
        while i < len(body):
            if body[i] != 0:
                raise ValueError("unexpected binary Engine.IO packet")
            i += 1
            length_digits = []
            while i < len(body) and body[i] != 0xFF:
                if body[i] > 9 or len(length_digits) > 8:
                    raise ValueError("invalid binary frame size")
                length_digits.append(str(body[i]))
                i += 1
            if i == len(body) or not length_digits:
                raise ValueError("truncated binary frame")
            i += 1
            size = int("".join(length_digits))
            if size > MAX_PACKET_BYTES or size > len(body) - i:
                raise ValueError("invalid binary frame length")
            packets.append(body[i:i + size].decode("utf-8"))
            i += size
        return packets
    source = body.decode("utf-8")
    packets = []
    pos = 0
    while pos < len(source):
        sep = source.find(":", pos)
        if sep < 0 or not source[pos:sep].isdigit():
            raise ValueError("malformed Engine.IO framing")
        count = int(source[pos:sep])
        if count > MAX_PACKET_BYTES:
            raise ValueError("oversized Engine.IO frame")
        pos = sep + 1
        begin, units = pos, 0
        while units < count and pos < len(source):
            units += 2 if ord(source[pos]) > 0xFFFF else 1
            pos += 1
        if units != count:
            raise ValueError("truncated Engine.IO frame")
        packets.append(source[begin:pos])
    return packets


def weapon_template(weapon_index: int, is_duck: bool = False) -> dict:
    """The client's 33 read fields; every numeric setting is PROVISIONAL."""
    magazine = 0 if is_duck else MAGAZINES[weapon_index]
    return {
        "weaponIndex": weapon_index, "ammo": magazine, "maxAmmo": magazine,
        "reloadSpeed": RELOADS_MS[weapon_index], "reloadTime": 0,
        "fireRate": FIRE_RATES_MS[weapon_index], "lastShot": 0,
        "dmg": 0 if is_duck else -DAMAGE[weapon_index],
        "bSpeed": 0.4, "bDist": 8, "bHeight": 9, "bWidth": 5,
        "bRandScale": None, "bSprite": 0, "bTrail": 0.0,
        "bounce": False, "bulletsPerShot": 1, "cAcc": 3,
        "camo": 0, "distBased": False, "explodeOnDeath": False,
        "front": True, "glowHeight": 0, "glowWidth": 0,
        "holdDist": 12, "length": 32, "maxLife": 800,
        "pierce": 0, "shake": 0, "spread": [0], "spreadIndex": 0,
        "width": 12, "yOffset": 14
    }


def map_fixture() -> dict:
    rgba = []
    for row in range(MAP_SIDE):
        for col in range(MAP_SIDE):
            boundary = min(col, row, MAP_SIDE - col - 1, MAP_SIDE - row - 1) < 2
            rgba.extend((0, 0, 0, 255) if boundary else (255, 255, 255, 255))
    return {
        "width": (MAP_SIDE - 4) * TILE,
        "height": (MAP_SIDE - 4) * TILE,
        "genData": {"width": MAP_SIDE, "height": MAP_SIDE, "data": {"data": rgba}},
        "clutter": [], "pickups": [],
        "gameMode": {
            "code": "ffa", "name": "Free For All", "teams": False,
            "score": 10, "desc1": "Experimental local FFA",
            "desc2": "Experimental local FFA"
        }
    }


@dataclass
class Participant:
    sid: str
    index: int
    name: str = "Player"
    class_index: int = 0
    x: float = 400.0
    y: float = 400.0
    angle: float = 0.0
    health: int = 100
    alive: bool = True
    score: int = 0
    kills: int = 0
    deaths: int = 0
    current_weapon: int = 0
    weapons: list[dict] = field(default_factory=list)
    reload_deadlines: dict[int, int] = field(default_factory=dict)
    fire_times: dict[int, int] = field(default_factory=dict)
    last_input_ts: int | None = None
    last_input_isn: int = -1
    spawned: bool = False

    def equip(self, class_index: int) -> None:
        self.class_index = class_index
        self.weapons = [
            weapon_template(w, is_duck=class_index == 8)
            for w in CLASS_WEAPONS[class_index]
        ]
        self.current_weapon = 0
        self.reload_deadlines.clear()
        self.fire_times.clear()

    def public(self) -> dict:
        """Reconstructed player shape consumed by the 2016 browser renderer."""
        return {
            "id": self.index, "index": self.index, "name": self.name,
            "room": ROOM, "team": "red" if self.index % 2 else "blue",
            "classIndex": self.class_index, "type": "player",
            "x": self.x, "y": self.y, "oldX": self.x, "oldY": self.y,
            "angle": self.angle, "width": 26, "height": 50, "speed": 0.18,
            "maxHealth": 100, "health": self.health, "score": self.score,
            "dead": not self.alive, "onScreen": True,
            "loggedIn": False, "account": {"clan": "", "rank": "", "hat": None},
            "kills": self.kills, "deaths": self.deaths, "likes": 0,
            "totalDamage": 0, "totalHealing": 0,
            "weapons": self.weapons, "currentWeapon": self.current_weapon,
            "isn": max(self.last_input_isn, 0), "spawnProtection": 0,
            "animIndex": 0, "frameCountdown": 0, "xSpeed": 0, "ySpeed": 0,
            "nameYOffset": 0, "jumpY": 0, "jumpDelta": 0,
            "jumpStrength": 0.72, "gravityStrength": 0.0042,
            "jumpCountdown": 0, "isBoss": False,
            "spray": {"info": {"scale": 30, "alpha": 1, "resolution": 30}, "src": ""}
        }


class Session:
    def __init__(self):
        self.sid = secrets.token_urlsafe(16)
        self.waiter = threading.Condition()
        self.messages: list[str] = []
        self.closed = False

    def packet(self, frame: str) -> None:
        with self.waiter:
            self.messages.append(frame)
            self.waiter.notify_all()

    def emit(self, event: str, *args: object) -> None:
        self.packet(event_frame(event, *args))

    def drain(self, wait=0.5) -> list[str]:
        with self.waiter:
            if not self.messages:
                self.waiter.wait(wait)
            data, self.messages = self.messages, []
            return data or ["6"]


class Arena:
    """Replaceable NEW gameplay adapter; not an implementation of old servers."""
    def __init__(self):
        self.lock = threading.RLock()
        self.sessions: dict[str, Session] = {}
        self.players: dict[str, Participant] = {}
        self.next_index = 3
        self.next_bullet = 1
        self.map = map_fixture()

    def connect(self) -> Session:
        with self.lock:
            session = Session()
            self.sessions[session.sid] = session
            return session

    def send(self, sid: str, event: str, *args: object) -> None:
        if sid in self.sessions:
            self.sessions[sid].emit(event, *args)

    def broadcast(self, event: str, *args: object, except_sid=None) -> None:
        for sid, player in self.players.items():
            if sid != except_sid and player.spawned:
                self.send(sid, event, *args)

    def disconnect(self, sid: str) -> None:
        with self.lock:
            session = self.sessions.pop(sid, None)
            if session:
                session.closed = True
                with session.waiter:
                    session.waiter.notify_all()
            player = self.players.pop(sid, None)
            if player and player.spawned:
                self.broadcast("rem", player.index)

    def _new_player(self, sid: str) -> Participant:
        player = self.players.get(sid)
        if player is None:
            idx = self.next_index
            self.next_index += 1
            player = Participant(sid, idx, x=min(400 + (idx - 3) * 60, 700))
            player.equip(0)
            self.players[sid] = player
        return player

    def _send_snapshot(self):
        for sid, recipient in self.players.items():
            if not recipient.spawned:
                continue
            flat = []
            for player in self.players.values():
                if not player.spawned:
                    continue
                flat.extend([
                    6, player.index, round(player.x), round(player.y),
                    round(player.angle),
                    max(player.last_input_isn, 0) if player is recipient else 0
                ])
            self.send(sid, "rsd", flat)

    def _reload(self, player: Participant, now_ms: int) -> None:
        slot = player.current_weapon
        weapon = player.weapons[slot]
        if not player.alive or slot in player.reload_deadlines:
            return
        if weapon["ammo"] >= weapon["maxAmmo"]:
            return
        player.reload_deadlines[slot] = now_ms + weapon["reloadSpeed"]

    def advance(self, now_ms: int) -> None:
        with self.lock:
            for player in self.players.values():
                if not player.alive:
                    player.reload_deadlines.clear()
                    continue
                done = [slot for slot, deadline in player.reload_deadlines.items()
                        if now_ms >= deadline]
                for slot in done:
                    player.weapons[slot]["ammo"] = player.weapons[slot]["maxAmmo"]
                    player.weapons[slot]["reloadTime"] = 0
                    del player.reload_deadlines[slot]
                    # Recovered one-positional-slot acknowledgement, private.
                    self.send(player.sid, "r", slot)

    def _shoot(self, shooter: Participant, args: list, now_ms: int) -> None:
        if not shooter.alive or len(args) != 6:
            return
        slot = shooter.current_weapon
        weapon = shooter.weapons[slot]
        if slot in shooter.reload_deadlines or weapon["ammo"] <= 0:
            return
        try:
            aim, distance = float(args[3]), float(args[4])
        except (ValueError, TypeError):
            return
        if not all(math.isfinite(x) for x in (aim, distance)) or distance < 0:
            return
        last = shooter.fire_times.get(slot)
        if last is not None and now_ms - last < weapon["fireRate"]:
            return
        shooter.fire_times[slot] = now_ms
        weapon["ammo"] -= 1
        bid = self.next_bullet
        self.next_bullet += 1
        self.broadcast("2", {
            "i": shooter.index, "x": shooter.x, "y": shooter.y,
            "s": slot, "d": aim, "si": bid,
        }, except_sid=shooter.sid)
        ux, uy = math.cos(aim), math.sin(aim)
        nearest = None
        for victim in self.players.values():
            if victim is shooter or not victim.alive or not victim.spawned:
                continue
            dx, dy = victim.x - shooter.x, victim.y - shooter.y
            along = dx * ux + dy * uy
            lateral = abs(dx * uy - dy * ux)
            if 0 <= along <= min(distance, 1200) and lateral <= 16:
                if nearest is None or along < nearest[0]:
                    nearest = (along, victim)
        if nearest is None:
            return
        target = nearest[1]
        dmg = min(DAMAGE[weapon["weaponIndex"]], target.health)
        if dmg <= 0 or shooter.class_index == 8:
            return
        target.health -= dmg
        self.broadcast("1", {
            # Recovered 2016 client handler treats gID as victim, dID as
            # the attacker; naming is counterintuitive, do not reverse.
            "gID": target.index, "dID": shooter.index,
            "amount": -dmg, "h": target.health,
            "bi": None, "dir": aim,
        })
        if target.health == 0:
            target.alive = False
            target.deaths += 1
            shooter.kills += 1
            shooter.score += 1
            target.reload_deadlines.clear()
            self.broadcast("3", {
                "gID": target.index, "dID": shooter.index,
                "kB": False, "kd": 1, "sS": 1, "ast": False,
            })
            self.broadcast("upd", {
                "i": shooter.index, "s": shooter.score, "kil": shooter.kills,
            })
            self.broadcast("upd", {
                "i": target.index, "dea": target.deaths,
            })
        if weapon["ammo"] == 0:
            # Client emits r separately after running out, after this fire.
            pass

    def event(self, sid: str, name: str, args: list, now_ms: int) -> None:
        with self.lock:
            if sid not in self.sessions:
                return
            if name == "ping1":
                self.send(sid, "pong1")
                return
            if name == "create":
                return
            if name == "respawn":
                player = self._new_player(sid)
                self.send(sid, "welcome", {"id": player.index, "room": ROOM}, False)
                return
            player = self.players.get(sid)
            if player is None:
                return
            if name == "gotit":
                if not args or not isinstance(args[0], dict):
                    return
                raw = args[0]
                try:
                    class_index = int(raw.get("classIndex", 0))
                except (ValueError, TypeError):
                    class_index = 0
                if not 0 <= class_index < len(CLASS_WEAPONS):
                    class_index = 0
                player.name = str(raw.get("name") or "Player")[:24]
                player.equip(class_index)
                player.health = 100
                player.alive = True
                player.last_input_isn = -1
                player.last_input_ts = None
                peer_list = [p.public() for p in self.players.values()
                             if p is not player and p.spawned]
                player.spawned = True
                setup = {
                    "mapData": self.map, "usersInRoom": peer_list, "you": player.public(),
                    "tileScale": TILE, "viewMult": 1,
                    "maxScreenWidth": 800, "maxScreenHeight": 600,
                }
                self.send(sid, "yourRoom", ROOM, "LOCAL")
                self.send(sid, "gameSetup", json.dumps(setup, separators=(",", ":")), True, True)
                self.broadcast("add", json.dumps(player.public()), except_sid=sid)
                self._send_snapshot()
            elif not player.spawned:
                return
            elif name == "sw" and args and type(args[0]) is int:
                slot = args[0]
                if 0 <= slot < len(player.weapons):
                    player.current_weapon = slot
                    self.broadcast("upd", {"i": player.index, "wi": slot}, except_sid=sid)
            elif name == "r":
                self._reload(player, now_ms)
            elif name == "1":
                self._shoot(player, args, now_ms)
            elif name == "4" and args and isinstance(args[0], dict):
                data = args[0]
                try:
                    h, v = float(data["hdt"]), float(data["vdt"])
                    seq, ts = int(data["isn"]), int(data["ts"])
                except (KeyError, ValueError, TypeError):
                    return
                if (h not in (-0.5, 0, 0.5) or v not in (-0.5, 0, 0.5)
                        or seq <= player.last_input_isn
                        or (player.last_input_ts is not None and ts < player.last_input_ts)):
                    return
                dt = 0 if player.last_input_ts is None else min(ts - player.last_input_ts, 100)
                player.last_input_ts = ts
                player.last_input_isn = seq
                magnitude = math.hypot(h, v)
                if magnitude and player.alive:
                    player.x = max(26, min(774, player.x + (h / magnitude) * 0.18 * dt))
                    player.y = max(26, min(774, player.y + (v / magnitude) * 0.18 * dt))
                self._send_snapshot()
            elif name == "ftc" and args:
                for target in self.players.values():
                    if target.spawned and target.index == args[0]:
                        self.send(sid, "add", json.dumps(target.public()))
                        break


class PvpHandler(AssetHandler):
    arena: Arena

    def do_GET(self):
        route = urlsplit(self.path)
        if route.path == "/socket.io/":
            return self.socket_get(route)
        return super().do_GET()

    def do_POST(self):
        route = urlsplit(self.path)
        if route.path != "/socket.io/":
            return self.send_bytes(404, b"", "text/plain")
        query = parse_qs(route.query)
        if query.get("transport") != ["polling"]:
            return self.send_bytes(400, b"", "text/plain")
        sid = query.get("sid", [""])[0]
        with self.arena.lock:
            if sid not in self.arena.sessions:
                return self.send_bytes(400, b'{"code":1}', "application/json")
        size = int(self.headers.get("Content-Length", "0"))
        if not 0 <= size <= MAX_PACKET_BYTES:
            return self.send_bytes(413, b"", "text/plain")
        try:
            for packet in decode_packets(self.rfile.read(size)):
                if packet == "2":
                    # Engine.IO pong is a raw type-3 packet, not Socket.IO.
                    session = self.arena.sessions.get(sid)
                    if session:
                        session.packet("3")
                elif packet == "1":
                    self.arena.disconnect(sid)
                elif packet.startswith("42"):
                    decoded = json.loads(packet[2:])
                    if (isinstance(decoded, list) and decoded
                            and isinstance(decoded[0], str) and len(decoded) <= 33):
                        self.arena.event(sid, decoded[0], decoded[1:], int(time.monotonic() * 1000))
        except (ValueError, UnicodeDecodeError, KeyError, IndexError, TypeError):
            return self.send_bytes(400, b"", "text/plain")
        return self.send_bytes(200, b"ok", "text/plain")

    def socket_get(self, route):
        q = parse_qs(route.query)
        if q.get("transport") != ["polling"]:
            return self.send_bytes(400, b'{"code":0}', "application/json")
        sid = q.get("sid", [None])[0]
        if sid is None:
            session = self.arena.connect()
            open_frame = "0" + json.dumps({
                "sid": session.sid, "upgrades": [],
                "pingInterval": 25000, "pingTimeout": 60000
            }, separators=(",", ":"))
            packets = [open_frame, "40"]
        else:
            with self.arena.lock:
                session = self.arena.sessions.get(sid)
            if not session or session.closed:
                return self.send_bytes(400, b'{"code":1}', "application/json")
            packets = session.drain()
        return self.send_bytes(200, encode_packets(packets), "text/plain; charset=UTF-8")

    def send_bytes(self, code, body, mime):
        self.send_response(code)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)


def serve(assets: ArchiveAssets, port: int) -> None:
    PvpHandler.assets = assets
    PvpHandler.socket_port = port
    PvpHandler.arena = Arena()
    server = ThreadingHTTPServer(("127.0.0.1", port), PvpHandler)
    server.daemon_threads = True
    def ticker():
        while not ticker_stop.wait(0.03):
            PvpHandler.arena.advance(int(time.monotonic() * 1000))
    ticker_stop = threading.Event()
    thread = threading.Thread(target=ticker, daemon=True)
    thread.start()
    print(f"Experimental original-client PvP: http://127.0.0.1:{port}/", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        ticker_stop.set()
        server.shutdown()
        server.server_close()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--port", type=int, default=8000)
    args = parser.parse_args()
    serve(ArchiveAssets(args.archive), args.port)


if __name__ == "__main__":
    main()

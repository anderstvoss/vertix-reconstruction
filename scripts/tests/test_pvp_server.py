"""Synthetic two-client EIO3 + FFA protocol regression, no original game data."""
import json
import threading
import unittest
from http.server import ThreadingHTTPServer
from urllib.request import Request, urlopen

from web.pvp_server import Arena, PvpHandler, decode_packets, encode_packets, event_frame


class WireTests(unittest.TestCase):
    def test_engine_io_text_framing_is_utf16_units(self):
        packets = ['42["cht","A🙂B"]', '42["r",0]', "2"]
        self.assertEqual(decode_packets(encode_packets(packets)), packets)

    def test_engine_io_xhr2_binary_framing(self):
        packet = b'42["ping1"]'
        payload = b"\x00" + bytes(int(char) for char in str(len(packet))) + b"\xff" + packet
        self.assertEqual(decode_packets(payload), [packet.decode()])
        with self.assertRaises(ValueError):
            decode_packets(payload[:-1])

    def test_bad_payload_rejected(self):
        for body in (b"garbage", b"3:4", b"99999999:", b"1:\xff"):
            with self.subTest(body=body), self.assertRaises((ValueError, UnicodeError)):
                decode_packets(body)


class ArenaTests(unittest.TestCase):
    def setUp(self):
        self.arena = Arena()
        self.left = self.arena.connect()
        self.right = self.arena.connect()

    def join(self, session, class_index, name):
        self.arena.event(session.sid, "respawn", [], 0)
        self.arena.event(session.sid, "gotit",
                         [{"name": name, "classIndex": str(class_index)}, False, 0, False], 0)

    def events(self, session, name):
        found = []
        for packet in session.messages:
            if packet.startswith("42"):
                event = json.loads(packet[2:])
                if event[0] == name:
                    found.append(event[1:])
        return found

    def test_two_clients_receive_distinct_setup_and_loadouts(self):
        self.join(self.left, 0, "A")
        self.join(self.right, 2, "B")
        left_setup = json.loads(self.events(self.left, "gameSetup")[0][0])
        right_setup = json.loads(self.events(self.right, "gameSetup")[0][0])
        self.assertEqual(left_setup["you"]["index"], 3)
        self.assertEqual(right_setup["you"]["index"], 4)
        self.assertEqual([w["weaponIndex"] for w in left_setup["you"]["weapons"]], [0, 5])
        self.assertEqual([w["weaponIndex"] for w in right_setup["you"]["weapons"]], [2, 7])
        self.assertEqual(len(left_setup["mapData"]["genData"]["data"]["data"]), 24 * 24 * 4)
        self.assertEqual([p["index"] for p in right_setup["usersInRoom"]], [3])
        self.assertEqual(json.loads(self.events(self.left, "add")[0][0])["index"], 4)

    def test_authoritative_damage_kill_targeting_and_reload_ack(self):
        self.join(self.left, 0, "A")
        self.join(self.right, 2, "B")
        shooter = self.arena.players[self.left.sid]
        target = self.arena.players[self.right.sid]
        self.assertEqual((shooter.x, shooter.y), (400.0, 400.0))
        self.assertEqual((target.x, target.y), (460.0, 400.0))
        for tick in range(5):
            self.arena.event(self.left.sid, "1",
                             [400, 400, 0, 0.0, 200.0, tick], 1000 + 143 * tick)
        self.assertEqual(target.health, 0)
        self.assertFalse(target.alive)
        self.assertEqual(shooter.score, 1)
        self.assertEqual(shooter.weapons[0]["ammo"], 19)
        self.assertEqual(len(self.events(self.right, "1")), 5)
        killed = self.events(self.right, "3")
        self.assertEqual(len(killed), 1)
        # gID = victim, dID = attacker (from first-party client).
        self.assertEqual(killed[0][0]["gID"], target.index)
        self.assertEqual(killed[0][0]["dID"], shooter.index)
        self.arena.event(self.left.sid, "r", [], 2000)
        self.arena.event(self.left.sid, "sw", [1], 2001)
        self.arena.advance(3499)
        self.assertEqual(self.events(self.left, "r"), [])
        self.arena.advance(3500)
        self.assertEqual(self.events(self.left, "r"), [[0]])
        self.assertEqual(self.events(self.right, "r"), [])
        self.arena.advance(3501)
        self.assertEqual(len(self.events(self.left, "r")), 1)
        self.assertEqual(shooter.weapons[0]["ammo"], 24)
        self.assertEqual(shooter.current_weapon, 1)

    def test_input_rejects_out_of_order_updates(self):
        self.join(self.left, 0, "A")
        player = self.arena.players[self.left.sid]
        self.arena.event(self.left.sid, "4", [{"hdt": 0, "vdt": -0.5,
                              "isn": 1, "ts": 1000, "s": 0}], 100)
        self.arena.event(self.left.sid, "4", [{"hdt": 0, "vdt": 0,
                              "isn": 2, "ts": 1010, "s": 0}], 110)
        self.assertEqual(player.y, 400.0)
        self.arena.event(self.left.sid, "4", [{"hdt": 0, "vdt": -0.5,
                              "isn": 3, "ts": 1020, "s": 0}], 120)
        self.assertAlmostEqual(player.y, 398.2)
        self.arena.event(self.left.sid, "4", [{"hdt": 0, "vdt": 0.5,
                              "isn": 2, "ts": 1030, "s": 0}], 130)
        self.assertAlmostEqual(player.y, 398.2)

    def test_disconnect_notifies_opponent(self):
        self.join(self.left, 0, "A")
        self.join(self.right, 1, "B")
        self.arena.disconnect(self.right.sid)
        self.assertEqual(self.events(self.left, "rem"), [[4]])
        self.assertNotIn(self.right.sid, self.arena.players)


class LoopbackProtocolTests(unittest.TestCase):
    def test_two_independent_eio3_http_sessions(self):
        class Handler(PvpHandler):
            arena = Arena()

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base = f"http://127.0.0.1:{server.server_port}/socket.io/?EIO=3&transport=polling"
        try:
            def get(url):
                with urlopen(url, timeout=5) as response:
                    return decode_packets(response.read())

            def post(sid, packet):
                request = Request(base + "&sid=" + sid, data=encode_packets([packet]),
                                  method="POST", headers={"Content-Type": "text/plain"})
                with urlopen(request, timeout=5) as response:
                    self.assertEqual(response.status, 200)

            left_open = get(base)
            right_open = get(base)
            self.assertEqual(left_open[1], "40")
            self.assertEqual(right_open[1], "40")
            sid1 = json.loads(left_open[0][1:])["sid"]
            sid2 = json.loads(right_open[0][1:])["sid"]
            self.assertNotEqual(sid1, sid2)
            post(sid1, event_frame("respawn"))
            post(sid2, event_frame("respawn"))
            welcome1 = get(base + "&sid=" + sid1)
            welcome2 = get(base + "&sid=" + sid2)
            self.assertEqual(json.loads(welcome1[0][2:])[0], "welcome")
            self.assertEqual(json.loads(welcome2[0][2:])[0], "welcome")
            post(sid1, event_frame("gotit", {"classIndex": "0", "name": "Alpha"}, False, 0, False))
            post(sid2, event_frame("gotit", {"classIndex": "1", "name": "Beta"}, False, 0, False))
            self.assertTrue(any('"gameSetup"' in pkt for pkt in get(base + "&sid=" + sid1)))
            self.assertTrue(any('"gameSetup"' in pkt for pkt in get(base + "&sid=" + sid2)))
            post(sid1, "2")
            self.assertEqual(get(base + "&sid=" + sid1), ["3"])
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


if __name__ == "__main__":
    unittest.main()

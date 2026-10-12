"""Static contract for the pending *real* two-original-client acceptance run.

This checks the acceptance script is internally consistent. It deliberately
does not turn synthetic state or unit tests into a PASS for the real game.
"""
import json
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parents[2]
FIXTURE = HERE / "tests/fixtures/ffa-original-client-acceptance.json"
EXPECTED = {
    ("server->client", "welcome"): 2,
    ("client->server", "gotit"): 4,
    ("client->server", "create"): 0,
    ("client->server", "respawn"): 0,
    ("server->client", "gameSetup"): 3,
    ("client->server", "4"): 1,
    ("server->client", "rsd"): 1,
    ("client->server", "1"): 6,
    ("server->client", "1"): 1,
    ("server->client", "3"): 1,
    ("server->client", "upd"): 1,
    ("server->client", "7"): 4,
    ("server->client", "8"): 1,
}


class OriginalClientAcceptanceContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))

    def test_all_source_derived_wire_arity_is_exact(self):
        observed = set()
        for phase in self.fixture["phases"]:
            for step in phase["steps"]:
                key = (step["direction"], step["event"])
                self.assertIn(key, EXPECTED)
                self.assertEqual(step["arity"], EXPECTED[key])
                self.assertTrue(step["assertion"])
                observed.add(key)
        self.assertEqual(observed, set(EXPECTED))
        self.assertEqual(
            self.fixture["original_client"]["sha256"],
            "cbab5cd590ff0d3a9d01a60eba835cd885b715988d0d87a7d287321399df2f09",
        )

    def test_fixtures_cannot_claim_live_browser_success(self):
        self.assertEqual(self.fixture["status"], "NOT_EXECUTED")
        self.assertEqual(self.fixture["server_rules"]["status"], "NEW_PROVISIONAL")
        self.assertEqual(
            self.fixture["historical_scope"], "RECOVERED_CLIENT_EVENT_SHAPES_ONLY"
        )
        self.assertIn("UNMODIFIED", self.fixture["acceptance"])
        self.assertGreaterEqual(len(self.fixture["required_artifacts"]), 8)
        self.assertGreaterEqual(len(self.fixture["negative_controls"]), 5)

    def test_all_milestones_are_represented_in_order(self):
        self.assertEqual(
            [p["phase"] for p in self.fixture["phases"]],
            [
                "transport-and-join",
                "map-and-player-state",
                "combat-and-respawn",
                "round-end-and-next-round",
            ],
        )
        for case in self.fixture["negative_controls"]:
            self.assertTrue(case["requirement"])


if __name__ == "__main__":
    unittest.main()

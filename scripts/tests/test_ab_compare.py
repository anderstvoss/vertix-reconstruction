import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import ab_compare as ab  # noqa: E402


def metrics(fps, p95, inputs, ping):
    return {
        "fps_mean": fps,
        "frame_ms": {"mean": 1000 / fps, "p50": 1000 / fps, "p95": p95, "p99": p95, "max": p95},
        "inputs_per_second": inputs,
        "updates_per_second": inputs,
        "ping_ms": ping,
        "dpi_scale": 1,
    }


class Compare(unittest.TestCase):
    def test_difference_and_which_is_better(self):
        rows = ab.compare_table(metrics(60, 20, 60, 10), metrics(120, 9, 60, 12))
        fps = next(r for r in rows if r.startswith("| Mean fps"))
        p95 = next(r for r in rows if r.startswith("| Frame time p95"))
        ping = next(r for r in rows if r.startswith("| Ping"))
        inputs = next(r for r in rows if r.startswith("| Inputs"))
        self.assertIn("+60.0 (Rust better)", fps)
        self.assertIn("-11.0 ms (Rust better)", p95)
        self.assertIn("+2.0 ms (KRP better)", ping)
        self.assertNotIn("better", inputs)

    def test_report_skips_pairs_without_both_runs(self):
        text = ab.report({"B": metrics(60, 20, 60, 5), "D": {"error": "no browser"}}, {"room": "DEV0"})
        self.assertIn("D, Rust client, browser: failed: no browser", text)
        self.assertIn("C, KRP client, desktop wrapper: not run", text)
        self.assertEqual(text.count("Not compared"), 2)
        self.assertIn("## Hands-on notes", text)

    def test_script_length(self):
        self.assertEqual(ab.script_ms("d:1500,s:800,a+w:1000,:500"), 3800)

    def test_server_comes_from_the_ports_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            f = Path(tmp) / "server.json"
            f.write_text('{"krp": "http://example:8083/", "classic": "http://example:8084/"}')
            self.assertEqual(ab.server_from_ports_file(f), "http://example:8083/")
            self.assertIsNone(ab.server_from_ports_file(Path(tmp) / "missing.json"))
            f.write_text("[]")
            self.assertIsNone(ab.server_from_ports_file(f))


if __name__ == "__main__":
    unittest.main()

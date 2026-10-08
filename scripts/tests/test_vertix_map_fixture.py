"""Offline tests for scripts/vertix_maps/png_to_gen_data.py; no original assets."""
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

CONVERTER = Path(__file__).resolve().parents[1] / "vertix_maps/png_to_gen_data.py"
spec = importlib.util.spec_from_file_location("png_to_gen_data", CONVERTER)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


def chunk(tag, body):
    return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body) & 0xffffffff)


def png(mode=2, corrupt=False):
    channels = 4 if mode == 6 else 3
    pixels = [40, 50, 60, 77][:channels]
    rows = (b"\x00" + bytes(pixels) * 5) * 5
    out = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", 5, 5, 8, mode, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
    if corrupt:
        out = out[:-6] + bytes([out[-6] ^ 1]) + out[-5:]
    return out


class ConverterTests(unittest.TestCase):
    def case(self, mode):
        d = tempfile.TemporaryDirectory()
        path = Path(d.name) / "synthetic.png"
        path.write_bytes(png(mode))
        return d, path

    def test_rgb_and_alpha_fill(self):
        d, path = self.case(2)
        try:
            fixture = mod.original_gen_data(path)
            self.assertEqual([fixture["width"], fixture["height"]], [5, 5])
            self.assertEqual(fixture["data"]["data"][:4], [40, 50, 60, 255])
            self.assertEqual(len(fixture["data"]["data"]), 100)
        finally:
            d.cleanup()

    def test_rgba_preserves_alpha(self):
        d, path = self.case(6)
        try:
            self.assertEqual(mod.original_gen_data(path)["data"]["data"][:4], [40, 50, 60, 77])
        finally:
            d.cleanup()

    def test_bad_crc_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "bad.png"
            path.write_bytes(png(corrupt=True))
            with self.assertRaises(ValueError):
                mod.decode_png(path)

    def test_cli_requires_explicit_substitute(self):
        d, path = self.case(2)
        try:
            good = [sys.executable, str(CONVERTER), str(path), "--map-data",
                    "--mode-name", "Hardpoint", "--score-to-win", "1000",
                    "--tile-scale", "256"]
            fixture = json.loads(subprocess.check_output(good, text=True))
            self.assertEqual(fixture["width"], 256)
            self.assertEqual(fixture["gameMode"]["score"], 1000)
            bad = subprocess.run([sys.executable, str(CONVERTER), str(path), "--map-data"],
                                 capture_output=True, text=True)
            self.assertNotEqual(bad.returncode, 0)
        finally:
            d.cleanup()


if __name__ == "__main__":
    unittest.main()

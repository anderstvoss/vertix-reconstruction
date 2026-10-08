#!/usr/bin/env python3
"""Build and validate all pinned KRP PNG -> 2016-client mapData fixtures.

The conversion is byte-faithful to the third-party PNGs, NOT proof that the
original server used those maps or the KRP mode rotation. Requires no network:
the caller supplies a KRP checkout containing server/maps/{mapN.png|N.png}.

Optional --original-source executes the *preserved* setupMap/canPlaceFlag
functions via probe_original_setupmap.cjs, not an independently ported decoder.
Without it, report status is STRUCTURAL_ONLY, never ORIGINAL_FUNCTION_PASS.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from pathlib import Path
import subprocess

from png_to_gen_data import original_gen_data

HERE = Path(__file__).resolve().parent
COLORS = {
    "black_pixels": (0, 0, 0),
    "white_pixels": (255, 255, 255),
    "red_pixels": (255, 0, 0),
    "blue_pixels": (0, 0, 255),
    "green_pixels": (0, 255, 0),
    "yellow_pixels": (255, 255, 0),
}


def load_csv(name: str):
    with (HERE / name).open(newline="", encoding="utf-8") as handle:
        return {int(r["map_id"]): r for r in csv.DictReader(handle)}


def git_blob_sha(data: bytes):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data, usedforsecurity=False).hexdigest()


def find_png(base: Path, number: int):
    for name in (f"map{number}.png", f"{number}.png"):
        path = base / name
        if path.is_file():
            return path
    raise FileNotFoundError(f"KRP map {number} absent under {base}")


def verify_png(rows: list[int], known: dict):
    total = len(rows) // 4
    if len(rows) % 4:
        raise AssertionError("RGBA array not divisible by 4")
    histogram = {}
    nonopaque = 0
    for i in range(0, len(rows), 4):
        rgb = tuple(rows[i:i+3])
        histogram[rgb] = histogram.get(rgb, 0) + 1
        nonopaque += rows[i+3] != 255
    for key, pixel in COLORS.items():
        assert histogram.get(pixel, 0) == int(known[key]), f"{key} count mismatch"
    assert total-sum(histogram.get(pixel, 0) for pixel in COLORS.values()) == int(known["other_pixels"])
    assert nonopaque == int(known["alpha_nonopaque"]), "PNG nonopaque alpha mismatch"
    return histogram


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("maps_dir", type=Path)
    p.add_argument("output_dir", type=Path)
    p.add_argument("--original-source", type=Path,
                   help="Preserved beautified August-06 app.js (external archive checkout)")
    p.add_argument("--node", default="node")
    p.add_argument("--tile-scale", type=int, default=256)
    p.add_argument("--synthetic-score", type=int, default=1000)
    args = p.parse_args()
    if args.tile_scale <= 0 or args.synthetic_score <= 0:
        p.error("Synthetic tile scale and score must be positive")
    locks = load_csv("krp_png_source_lock.csv")
    assignment = load_csv("krp_mode_hypotheses.csv")
    if set(locks) != set(range(24)) or set(assignment) != set(range(24)):
        raise AssertionError("Expected exact map IDs 0..23 in both source-lock files")
    if args.original_source and not args.original_source.is_file():
        p.error("Original 2016 JS source file does not exist")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    summary = []
    for map_id in range(24):
        lock = locks[map_id]
        path = find_png(args.maps_dir, map_id)
        pngbytes = path.read_bytes()
        actual_sha = git_blob_sha(pngbytes)
        assert actual_sha == lock["git_blob_sha"], f"Map {map_id} source SHA mismatch"
        gen = original_gen_data(path)
        assert gen["width"] == int(lock["width"]) and gen["height"] == int(lock["height"])
        assert len(gen["data"]["data"]) == 4*gen["width"]*gen["height"]
        hist = verify_png(gen["data"]["data"], lock)
        first = " ".join(map(str, gen["data"]["data"][:3]))
        assert first == lock["first_pixel_rgb"], f"first pixel changed: map {map_id}"
        labels = assignment[map_id]["krp_mode_names"].split("+")
        codes = assignment[map_id]["krp_mode_codes"].split("+")
        assert len(labels) == len(codes) and labels
        for code, name in zip(codes, labels):
            # Deliberately NOT a recovered gameSetup; explicit synthetic values.
            fixture = {
                "genData": gen,
                "width": (gen["width"]-4)*args.tile_scale,
                "height": (gen["height"]-4)*args.tile_scale,
                "gameMode": {"name": name, "score": args.synthetic_score},
                "tiles": [], "clutter": [], "pickups": [],
            }
            dst = args.output_dir / f"map{map_id:02d}-{code}.mapData.json"
            dst.write_text(json.dumps(fixture, separators=(",", ":"))+"\n", encoding="utf-8")
            walls = hist.get((0,0,0), 0) + (first != "0 0 0")
            hardpoints = hist.get((255,255,0), 0) if name in ("Hardpoint", "Zone War") else 0
            row = {
                "map_id": map_id, "mode_code": code, "mode_name": name,
                "source_git_blob_sha": actual_sha,
                "tile_count": gen["width"]*gen["height"],
                "expected_wall_count": walls,
                "expected_hardpoint_count": hardpoints,
                "fixture_file": dst.name,
                "original_setupMap_test": "NOT_RUN_NO_SOURCE",
                "original_functions_execution": False,
                "synthetic_score": args.synthetic_score,
                "synthetic_tile_scale": args.tile_scale,
            }
            if args.original_source:
                command = [args.node, str(HERE / "probe_original_setupmap.cjs"),
                           str(args.original_source), str(dst), str(args.tile_scale)]
                completed = subprocess.run(command, capture_output=True, text=True)
                if completed.returncode:
                    raise AssertionError(
                        f"Original setupMap failed map {map_id}/{code}:\n"
                        f"{completed.stdout}\n{completed.stderr}"
                    )
                output = json.loads(completed.stdout)
                assert output["tiles"] == row["tile_count"]
                assert output["walls"] == walls
                assert output["hardpoints"] == hardpoints
                assert output["pass"] is True
                row["original_setupMap_test"] = "PASS"
                row["original_functions_execution"] = True
            summary.append(row)
    report = {
        "classification": "UNVERIFIED_THIRD_PARTY_MAP_CANDIDATES",
        "verified_png_count": 24,
        "fixture_cases": len(summary),
        "original_source_supplied": args.original_source is not None,
        "historical_mode_or_score_asserted": False,
        "results": summary,
    }
    (args.output_dir / "manifest.json").write_text(
        json.dumps(report, indent=2)+"\n", encoding="utf-8")
    print(json.dumps({
        "pass": True, "pngs": 24, "cases": len(summary),
        "original_cases_executed": sum(r["original_functions_execution"] for r in summary),
        "source_locked": True, "output": str(args.output_dir),
    }))


if __name__ == "__main__":
    main()

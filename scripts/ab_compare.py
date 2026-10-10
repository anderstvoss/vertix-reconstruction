#!/usr/bin/env python3
"""Compare the clients on one server: KRP's client against the Rust port,
in the browser (B vs D) and on the desktop (C vs E).

    python3 scripts/ab_compare.py --server http://HOST:8080 --room DEV0

Each client plays the same scripted input in the same room for the same
time, one after another, and reports its own metrics (frame times, inputs
and server updates per second, ping). The results, screenshots and a
report with both comparisons and a hands-on checklist go to
out/ab/<time>/.

  B  KRP's client in a browser, measured by desktop/krp/inject.js
  C  KRP's client in the desktop wrapper (desktop/krp)
  D  the Rust client's browser build (/rust/ on the server)
  E  the Rust client's desktop build

By default every client runs as KRP does (one input per frame, CSS-pixel
display), so differences come from the clients rather than their settings.
`--input 60` and `--display sharp` apply the reconstruction's options to
all four. KRP's client draws and sends input in one loop, so its input rate
caps that loop; the Rust client sends input on its own tick and keeps
drawing every frame.

Browser runs need Playwright (see scripts/ab/browser.mjs). Desktop runs use
release builds: `cargo build --release` and
`cargo build --release --manifest-path desktop/krp/Cargo.toml`. Standard
library only.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import urlencode

ROOT = Path(__file__).resolve().parents[1]
EXE = ".exe" if sys.platform == "win32" else ""

CLIENTS = {
    "B": "KRP client, browser",
    "C": "KRP client, desktop wrapper",
    "D": "Rust client, browser",
    "E": "Rust client, desktop",
}
PAIRS = [("B", "D", "Browser"), ("C", "E", "Desktop")]

# (label, how to read it from a metrics dict, unit, lower is better)
ROWS = [
    ("Mean fps", lambda m: m["fps_mean"], "", False),
    ("Frame time mean", lambda m: m["frame_ms"]["mean"], "ms", True),
    ("Frame time p50", lambda m: m["frame_ms"]["p50"], "ms", True),
    ("Frame time p95", lambda m: m["frame_ms"]["p95"], "ms", True),
    ("Frame time p99", lambda m: m["frame_ms"]["p99"], "ms", True),
    ("Frame time max", lambda m: m["frame_ms"]["max"], "ms", True),
    ("Inputs sent per second", lambda m: m["inputs_per_second"], "", None),
    ("Server updates per second", lambda m: m["updates_per_second"], "", None),
    ("Ping", lambda m: m["ping_ms"], "ms", True),
    ("Device pixel ratio", lambda m: m["dpi_scale"], "", None),
]

CHECKLIST = """\
## Hands-on notes

Play each pair side by side for a few minutes and note what differs. The
numbers above cannot show these.

| Question | {a} | {b} |
| --- | --- | --- |
| Movement: does stopping and turning feel immediate? | | |
| Aim: does the gun follow the mouse without lag? | | |
| Jumping: same timing and height? | | |
| Shooting: same fire rate, recoil and hit feedback? | | |
| Other players: do they move smoothly or jump around? | | |
| Sharpness of the map, sprites and text | | |
| HUD, chat and scoreboard: same size and placement? | | |
| Start menu and class picker | | |
| Anything missing or wrong | | |
"""


def fmt(v: float) -> str:
    return f"{v:.0f}" if abs(v) >= 100 else f"{v:.1f}"


def compare_table(a: dict, b: dict) -> list[str]:
    """Rows comparing metrics `a` and `b`; the difference is b minus a."""
    lines = ["| Metric | KRP | Rust | Difference |", "| --- | --- | --- | --- |"]
    for label, get, unit, lower_better in ROWS:
        try:
            va, vb = float(get(a)), float(get(b))
        except (KeyError, TypeError, ValueError):
            continue
        diff = vb - va
        note = ""
        if lower_better is not None and abs(diff) > 1e-9:
            better = diff < 0 if lower_better else diff > 0
            note = " (Rust better)" if better else " (KRP better)"
        sign = "+" if diff > 0 else ""
        u = f" {unit}" if unit else ""
        lines.append(f"| {label} | {fmt(va)}{u} | {fmt(vb)}{u} | {sign}{fmt(diff)}{u}{note} |")
    return lines


def report(results: dict, settings: dict) -> str:
    out = ["# Client comparison", ""]
    out.append(
        "Settings: "
        + ", ".join(f"{k} `{v}`" for k, v in settings.items() if v not in (None, ""))
        + "."
    )
    out.append("")
    for key, name in CLIENTS.items():
        r = results.get(key)
        state = "not run" if r is None else ("failed: " + r["error"] if "error" in r else "ran")
        out.append(f"- {key}, {name}: {state}")
    out.append("")
    for a, b, title in PAIRS:
        ra, rb = results.get(a), results.get(b)
        out.append(f"## {title}: {CLIENTS[a]} ({a}) vs {CLIENTS[b]} ({b})")
        out.append("")
        if not ra or not rb or "error" in ra or "error" in rb:
            out.append("Not compared: both clients need a successful run.")
            out.append("")
            continue
        out += compare_table(ra, rb)
        out.append("")
        shots = [f"![{k}]({k}.png)" for k in (a, b) if (results.get("_dir") or Path()).joinpath(f"{k}.png").exists()]
        if shots:
            out.append(" ".join(shots))
            out.append("")
    out.append(CHECKLIST.format(a="KRP", b="Rust"))
    return "\n".join(out)


def run(cmd: list[str], timeout: float) -> dict:
    """Runs a client that prints its metrics JSON as its last stdout line."""
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, cwd=ROOT)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {"error": str(e)}
    lines = [line for line in p.stdout.splitlines() if line.startswith("{")]
    if not lines:
        tail = (p.stderr or p.stdout).strip().splitlines()[-3:]
        return {"error": f"exit {p.returncode}: " + " / ".join(tail)}
    return json.loads(lines[-1])


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--server", required=True, help="reconstruction server, e.g. http://HOST:8080")
    ap.add_argument("--room", default="DEV0")
    ap.add_argument("--clients", default="B,C,D,E", help="which clients to run, e.g. B,D")
    ap.add_argument("--duration", type=float, default=30, help="seconds per client")
    ap.add_argument("--script", default="d:1500,s:800,a+w:1000,space:200,d+s:1500,:500",
                    help="scripted keys, repeated until the time is up")
    ap.add_argument("--input", default="frame", help="'frame' or a rate in Hz, for every client")
    ap.add_argument("--display", default="krp", help="'krp' or 'sharp', for every client")
    ap.add_argument("--shot-at", type=float, default=10, help="seconds in to take screenshots")
    ap.add_argument("--channel", default="", help="browser to use, e.g. chrome or msedge")
    ap.add_argument("--node", default="node")
    ap.add_argument("--out", default=str(ROOT / "out" / "ab"))
    args = ap.parse_args()

    stamp = time.strftime("%Y%m%d-%H%M%S")
    out = Path(args.out) / stamp
    out.mkdir(parents=True, exist_ok=True)
    server = args.server.rstrip("/")
    reps = max(1, int(args.duration * 1000 // max(1, script_ms(args.script))) + 1)
    script = ",".join([args.script] * reps)
    limit = args.duration + 120

    def browser(url: str, shot: Path, inject: Path | None = None, shell: dict | None = None) -> dict:
        spec = {"url": url, "duration": args.duration, "screenshot": str(shot),
                "shotAt": args.shot_at, "channel": args.channel}
        if inject:
            spec |= {"inject": str(inject), "shell": shell}
        return run([args.node, str(ROOT / "scripts" / "ab" / "browser.mjs"), json.dumps(spec)], limit)

    common = {"input": args.input, "display": args.display, "autoplay": True,
              "script": script, "duration": str(args.duration)}
    results: dict = {}
    for key in [k.strip().upper() for k in args.clients.split(",") if k.strip()]:
        name = f"AB-{key}"
        shot = out / f"{key}.png"
        print(f"{key}: {CLIENTS.get(key, '?')} ...", flush=True)
        if key == "B":
            r = browser(f"{server}/?{args.room}", shot, ROOT / "desktop" / "krp" / "inject.js",
                        common | {"name": name, "label": "browser"})
        elif key == "C":
            exe = ROOT / "desktop" / "krp" / "target" / "release" / f"vertix-krp-desktop{EXE}"
            cmd = [str(exe), "--server", server, "--room", args.room, "--name", name, "--autoplay"]
            for k in ("input", "display", "script", "duration"):
                cmd += [f"--{k}", str(common[k])]
            r = run(cmd, limit)
        elif key == "D":
            q = urlencode(common | {"room": args.room, "name": name, "autoplay": "1"})
            r = browser(f"{server}/rust/?{q}", shot)
        elif key == "E":
            exe = ROOT / "target" / "release" / f"vertix-client{EXE}"
            cmd = [str(exe), "--server", server, "--room", args.room, "--name", name, "--autoplay", "1",
                   "--screenshot", str(shot), "--shot-at", str(args.shot_at)]
            for k in ("input", "display", "script", "duration"):
                cmd += [f"--{k}", str(common[k])]
            r = run(cmd, limit)
        else:
            print(f"  unknown client {key}", file=sys.stderr)
            continue
        results[key] = r
        (out / f"{key}.json").write_text(json.dumps(r, indent=2) + "\n")
        print("  " + (r["error"] if "error" in r else f"{r['fps_mean']:.1f} fps"), flush=True)

    settings = {"room": args.room, "seconds each": args.duration, "input": args.input,
                "display": args.display, "browser": args.channel or "Playwright Chromium"}
    text = report(results | {"_dir": out}, settings)
    (out / "report.md").write_text(text + "\n")
    print(f"report: {out / 'report.md'}")
    if shutil.which(args.node) is None and any(k in results for k in "BD"):
        print("note: browser runs need Node.js and Playwright", file=sys.stderr)
    return 0 if all("error" not in r for r in results.values()) else 1


def script_ms(script: str) -> float:
    total = 0.0
    for step in script.split(","):
        _, _, ms = step.partition(":")
        try:
            total += float(ms)
        except ValueError:
            pass
    return total


if __name__ == "__main__":
    sys.exit(main())

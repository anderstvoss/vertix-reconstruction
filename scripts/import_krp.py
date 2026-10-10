#!/usr/bin/env python3
"""Convert KrunkerRevival's game data (TypeScript object literals) to JSON.

KrunkerRevivalProject/vertix (KRP) is the baseline this reconstruction
ports. Its classes, weapons, modes and cosmetic catalogues are plain object
literals in `core/src/*.ts`. This reads them as data (it never runs KRP's
code) and writes `data/krp/*.json`, recording the KRP commit they came from.

    python3 scripts/import_krp.py PATH/TO/KRP-CHECKOUT

The checkout is a clone of the archive's mirror
(`vertix-preservation/mirrors/KrunkerRevivalProject-vertix.git`).
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
from pathlib import Path

# (output file, [(source file, exported const, output key)])
TABLES = {
    "loadouts.json": [
        ("core/src/loadouts.ts", "characterClasses", "classes"),
        ("core/src/loadouts.ts", "weapons", "weapons"),
    ],
    "gamemodes.json": [("core/src/gamemodes.ts", "gameModes", "modes")],
    "skins.json": [
        ("core/src/skins.ts", "hats", "hats"),
        ("core/src/skins.ts", "shirts", "shirts"),
        ("core/src/skins.ts", "camos", "camos"),
    ],
    "sprays.json": [("core/src/sprays.ts", "sprays", "sprays")],
}


class Parser:
    """Reads one JavaScript literal: objects, arrays, strings, numbers,
    true/false/null/undefined, with comments and trailing commas."""

    def __init__(self, text: str, pos: int):
        self.s = text
        self.i = pos

    def error(self, msg: str) -> ValueError:
        line = self.s.count("\n", 0, self.i) + 1
        return ValueError(f"line {line}: {msg}")

    def skip(self) -> None:
        while self.i < len(self.s):
            c = self.s[self.i]
            if c.isspace():
                self.i += 1
            elif self.s.startswith("//", self.i):
                end = self.s.find("\n", self.i)
                self.i = len(self.s) if end < 0 else end
            elif self.s.startswith("/*", self.i):
                end = self.s.find("*/", self.i)
                if end < 0:
                    raise self.error("unterminated comment")
                self.i = end + 2
            else:
                return

    def peek(self) -> str:
        self.skip()
        if self.i >= len(self.s):
            raise self.error("unexpected end")
        return self.s[self.i]

    def value(self):
        c = self.peek()
        if c == "{":
            return self.obj()
        if c == "[":
            return self.arr()
        if c in "\"'`":
            return self.string()
        if c == "-" or c == "." or c.isdigit() or self.s.startswith("Math.", self.i):
            return self.expr()
        word = self.ident()
        consts = {"true": True, "false": False, "null": None, "undefined": None}
        if word in consts:
            return consts[word]
        raise self.error(f"not a literal: {word!r}")

    def ident(self) -> str:
        self.skip()
        start = self.i
        while self.i < len(self.s) and (self.s[self.i].isalnum() or self.s[self.i] in "_$"):
            self.i += 1
        if start == self.i:
            raise self.error(f"unexpected {self.s[self.i]!r}")
        return self.s[start : self.i]

    def string(self) -> str:
        quote = self.s[self.i]
        self.i += 1
        out = []
        while True:
            if self.i >= len(self.s):
                raise self.error("unterminated string")
            c = self.s[self.i]
            if c == quote:
                self.i += 1
                return "".join(out)
            if quote == "`" and self.s.startswith("${", self.i):
                raise self.error("template expression")
            if c == "\\":
                nxt = self.s[self.i + 1]
                if nxt == "u":
                    out.append(chr(int(self.s[self.i + 2 : self.i + 6], 16)))
                    self.i += 6
                    continue
                out.append({"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(nxt, nxt))
                self.i += 2
                continue
            out.append(c)
            self.i += 1

    def expr(self):
        """Products and quotients of numbers and `Math.PI`, as in a
        spread table (`-Math.PI / 2`)."""
        value = self.unary()
        while self.peek() in "*/":
            op = self.s[self.i]
            self.i += 1
            rhs = self.unary()
            value = value * rhs if op == "*" else value / rhs
        return value

    def unary(self):
        if self.peek() == "-":
            self.i += 1
            return -self.unary()
        if self.s.startswith("Math.PI", self.i):
            self.i += len("Math.PI")
            return math.pi
        return self.number()

    def number(self):
        start = self.i
        while self.i < len(self.s) and (self.s[self.i].isalnum() or self.s[self.i] in "._"):
            self.i += 1
        raw = self.s[start : self.i].replace("_", "")
        try:
            return int(raw, 0)
        except ValueError:
            return float(raw)

    def obj(self) -> dict:
        self.i += 1
        out = {}
        while True:
            if self.peek() == "}":
                self.i += 1
                return out
            key = self.string() if self.peek() in "\"'" else self.ident()
            if self.peek() != ":":
                raise self.error(f"expected ':' after {key!r}")
            self.i += 1
            out[key] = self.value()
            if self.peek() == ",":
                self.i += 1

    def arr(self) -> list:
        self.i += 1
        out = []
        while True:
            if self.peek() == "]":
                self.i += 1
                return out
            out.append(self.value())
            if self.peek() == ",":
                self.i += 1


def read_const(text: str, name: str):
    marker = f"export const {name}"
    at = text.find(marker)
    if at < 0:
        raise ValueError(f"{name} not found")
    eq = text.find("=", at + len(marker))
    return Parser(text, eq + 1).value()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("krp", type=Path, help="KRP checkout")
    ap.add_argument("--out", type=Path, default=Path(__file__).resolve().parents[1] / "data/krp")
    args = ap.parse_args()

    commit = subprocess.run(
        ["git", "-C", str(args.krp), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    args.out.mkdir(parents=True, exist_ok=True)
    for out_name, parts in TABLES.items():
        doc = {
            "source": {
                "repository": "KrunkerRevivalProject/vertix",
                "commit": commit,
                "files": sorted({f for f, _, _ in parts}),
                "note": "Converted from KRP's TypeScript by scripts/import_krp.py; do not edit by hand.",
            }
        }
        for src, const, key in parts:
            doc[key] = read_const((args.krp / src).read_text(encoding="utf-8"), const)
        path = args.out / out_name
        path.write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
        sizes = ", ".join(f"{k} {len(doc[k])}" for _, _, k in parts)
        print(f"{path.name}: {sizes}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

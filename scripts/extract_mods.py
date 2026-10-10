#!/usr/bin/env python3
"""Unpack the mod packs that the archive holds inside a git bundle.

Most community mod packs survive only in a fan repository that the archive
keeps as a git bundle, which the server cannot read directly. This script
reads that bundle through a throwaway clone and writes each pack listed in
data/content/mods.json to <out>/<key>/vertixmod.zip, checking every pack
against its recorded SHA-256. The output directory is never committed.
Packs the archive holds as plain files (Wayback captures) need no
unpacking; the server reads them from the archive.

    python3 scripts/extract_mods.py --archive ../vertix-archive [--out content/mods]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "data" / "content" / "mods.json"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--archive", required=True, type=Path)
    ap.add_argument("--out", type=Path, default=ROOT / "content" / "mods")
    args = ap.parse_args()

    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    wanted = [
        (pack["key"], src)
        for pack in manifest["packs"]
        for src in pack["sources"]
        if src["source"]["kind"] == "bundle"
    ]
    bundles = sorted({src["source"]["bundle"] for _, src in wanted})
    written = 0
    for rel in bundles:
        bundle = args.archive / rel
        if bundle.read_bytes()[:64].startswith(b"version https://git-lfs.github.com/spec/"):
            sys.exit("%s is a Git LFS pointer; run `git lfs pull` in the archive" % rel)
        with tempfile.TemporaryDirectory() as tmp:
            clone = Path(tmp) / "clone"
            subprocess.run(["git", "clone", "--quiet", "--no-checkout", str(bundle), str(clone)], check=True)
            for key, src in wanted:
                if src["source"]["bundle"] != rel:
                    continue
                data = subprocess.run(
                    ["git", "-C", str(clone), "cat-file", "blob", src["source"]["blob"]],
                    capture_output=True, check=True,
                ).stdout
                if hashlib.sha256(data).hexdigest() != src["sha256"]:
                    sys.exit("%s: SHA-256 differs from data/content/mods.json" % key)
                dest = args.out / key / "vertixmod.zip"
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(data)
                written += 1
    print("unpacked %d mod packs into %s" % (written, args.out))
    return 0


if __name__ == "__main__":
    sys.exit(main())

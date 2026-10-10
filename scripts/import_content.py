#!/usr/bin/env python3
"""Index the archive's cosmetic art and mod packs for the server.

Nothing from the archive is copied into this repository. This script writes
two code-free indexes that say where each file lives in a vertix-archive
clone and what its SHA-256 is; the server reads and checks the files
themselves at start-up, the same way it loads maps.

- data/content/cosmetics.json: every first-party version (Wayback captures,
  grade A, and the Aug-2016 Android APK, grade B) of the hats, shirts, camos
  and sprays in the archive's asset database, with the dates it was seen.
  The server picks one version per file by date ([content] date).
- data/content/mods.json: the community mod packs (vertixmod.zip) the
  archive holds: the two Wayback captures of Dropbox packs, and the packs
  in the JeanPaulDot/VERTIX fan repository's git bundle. Bundle packs must
  be unpacked first with scripts/extract_mods.py.

    python3 scripts/import_content.py --archive ../vertix-archive [--check]

--check exits 1 if the files in data/content differ from a fresh import.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "data" / "content"
ASSET_DB = "vertix-preservation/derived/assets/assets.json"
SUMS = "vertix-preservation/manifests/sha256sums.txt"

# Cosmetic families: asset-database key prefix -> the URL paths the KRP
# client requests for a key under it. Hats, shirts and camos are requested
# at their original paths; KRP's client loads sprays from /assets/sprays/
# (the path the server's spray list names), so they are served at both.
FAMILIES = {
    "images/hats/": ["/{key}"],
    "images/shirts/": ["/{key}"],
    "images/camos/": ["/{key}"],
    "images/sprays/": ["/{key}", "/assets/sprays/{name}"],
}
FIRST_PARTY = {"A", "B"}

# Mod packs saved by the Wayback Machine from Dropbox, keyed by the
# Dropbox share key the client's mod box accepts.
WAYBACK_PACKS = "vertix-preservation/originals/wayback"
JPD_BUNDLE = "vertix-preservation/originals/github/JeanPaulDot-VERTIX.bundle"
JPD_MODS = "data/mods"
# The stock res.zip's placeholder sound: 332 bytes, never played.
STUB_SOUND_BYTES = 332


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_sums(archive: Path) -> dict[str, str]:
    sums = {}
    for line in (archive / SUMS).read_text(encoding="utf-8").splitlines():
        digest, _, path = line.partition("  ")
        if path:
            sums[path.strip()] = digest.strip().lower()
    return sums


def verified(archive: Path, sums: dict[str, str], rel: str) -> bytes:
    data = (archive / rel).read_bytes()
    if data.startswith(b"version https://git-lfs.github.com/spec/"):
        sys.exit("%s is a Git LFS pointer; run `git lfs pull` in the archive" % rel)
    if sums.get(rel) != sha256(data):
        sys.exit("%s does not match sha256sums.txt" % rel)
    return data


def archive_commit(archive: Path) -> str:
    out = subprocess.run(["git", "-C", str(archive), "rev-parse", "HEAD"], capture_output=True, text=True)
    return out.stdout.strip() if out.returncode == 0 else "unknown"


def pick_location(version: dict) -> dict | None:
    """The first-party copy the server reads: a loose file before a zip member."""
    occ = [o for o in version["occurrences"] if o.get("grade") in FIRST_PARTY]
    occ.sort(key=lambda o: ("::" in o["location"], o["date"], o["location"]))
    if not occ:
        return None
    container, _, member = occ[0]["location"].partition("::")
    return {"archive_path": container, "member": member or None, "source": occ[0]["source"]}


def build_cosmetics(archive: Path, sums: dict[str, str]) -> dict:
    db_bytes = verified(archive, sums, ASSET_DB)
    db = json.loads(db_bytes)
    files = []
    counts: dict[str, int] = {}
    for key in sorted(db["assets"], key=natural):
        prefix = next((p for p in FAMILIES if key.startswith(p)), None)
        if prefix is None:
            continue
        versions = []
        for v in db["assets"][key]["versions"]:
            if v.get("evidence") != "RECOVERED" or "placeholder" in " ".join(v.get("flags", [])):
                continue
            loc = pick_location(v)
            if loc is None:
                continue
            seen = v["first_party_seen"]
            versions.append({
                "sha256": v["sha256"],
                "bytes": v["bytes"],
                "first_seen": seen[0][:10],
                "last_seen": seen[-1][:10],
                **loc,
            })
        if not versions:
            continue
        versions.sort(key=lambda v: (v["first_seen"], v["sha256"]))
        name = key[len(prefix):]
        paths = [t.format(key=key, name=name) for t in FAMILIES[prefix]]
        family = prefix.split("/")[1]
        counts[family] = counts.get(family, 0) + 1
        files.append({"key": key, "family": family, "paths": paths, "versions": versions})
    return {
        "source": {
            "archive_commit": archive_commit(archive),
            "asset_db": ASSET_DB,
            "asset_db_sha256": sha256(db_bytes),
            "note": "Generated by scripts/import_content.py; do not edit by hand. "
                    "Only first-party versions (grade A Wayback, grade B Android APK) are listed.",
        },
        "counts": counts,
        "files": files,
    }


def natural(s: str) -> list:
    return [int(t) if t.isdigit() else t for t in re.split(r"(\d+)", s)]


def slug(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


def pack_facts(data: bytes) -> dict:
    """Counts that describe a pack without copying any of its content."""
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        infos = [i for i in z.infolist() if not i.is_dir()]
        sounds = set()
        for i in infos:
            name = i.filename.replace("vertixmod/", "", 1)
            if name.startswith("sounds/") and not name.endswith(".DS_Store") and i.file_size != STUB_SOUND_BYTES:
                sounds.add(name.rsplit(".", 1)[0][len("sounds/"):])
        sprites = sum(1 for i in infos if i.filename.replace("vertixmod/", "", 1).startswith("sprites/"))
    return {"members": len(infos), "sprites": sprites, "real_sounds": len(sounds)}


def wayback_packs(archive: Path, sums: dict[str, str]) -> list[dict]:
    out = []
    pattern = re.compile(r"^%s/(\d{14})/dl\.dropboxusercontent\.com/s/([^/]+)/vertixmod\.zip\.[A-Z0-9]+$" % re.escape(WAYBACK_PACKS))
    for rel in sorted(sums):
        m = pattern.match(rel)
        if not m:
            continue
        data = verified(archive, sums, rel)
        out.append({
            "capture": m.group(1),
            "dropbox_key": m.group(2),
            "source": {"kind": "archive", "archive_path": rel},
            "sha256": sha256(data),
            "bytes": len(data),
            **pack_facts(data),
        })
    return out


def bundle_packs(archive: Path, sums: dict[str, str]) -> tuple[str, list[dict]]:
    verified(archive, sums, JPD_BUNDLE)
    with tempfile.TemporaryDirectory() as tmp:
        clone = Path(tmp) / "clone"
        subprocess.run(["git", "clone", "--quiet", "--no-checkout", str(archive / JPD_BUNDLE), str(clone)], check=True)
        head = subprocess.run(["git", "-C", str(clone), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
        listing = subprocess.run(["git", "-C", str(clone), "ls-tree", "-r", "HEAD", "--", JPD_MODS],
                                 capture_output=True, text=True, check=True).stdout
        out = []
        for line in listing.splitlines():
            meta, _, path = line.partition("\t")
            if not path.endswith("/vertixmod.zip"):
                continue
            blob = meta.split()[2]
            data = subprocess.run(["git", "-C", str(clone), "cat-file", "blob", blob], capture_output=True, check=True).stdout
            name = path[len(JPD_MODS) + 1:-len("/vertixmod.zip")]
            out.append({
                "name": name,
                "source": {"kind": "bundle", "bundle": JPD_BUNDLE, "commit": head, "path": path, "blob": blob},
                "sha256": sha256(data),
                "bytes": len(data),
                **pack_facts(data),
            })
    return head, out


def build_mods(archive: Path, sums: dict[str, str]) -> dict:
    wayback = wayback_packs(archive, sums)
    head, bundled = bundle_packs(archive, sums)
    packs = []
    for p in sorted(bundled, key=lambda p: p["name"].lower()):
        packs.append({
            "key": slug(p["name"]),
            "name": p["name"],
            "aliases": [],
            "sources": [{k: p[k] for k in ("source", "sha256", "bytes", "members", "sprites", "real_sounds")}],
        })
    # A Wayback capture of a pack's Dropbox original goes first, ahead of
    # the fan repository's repack (CAPTURE_OF below).
    for w in wayback:
        entry = {k: w[k] for k in ("source", "sha256", "bytes", "members", "sprites", "real_sounds")}
        entry["source"] = dict(entry["source"], capture=w["capture"])
        match = next((p for p in packs if p["key"] == CAPTURE_OF.get(w["dropbox_key"])), None)
        if match is None:
            match = {"key": w["dropbox_key"], "name": "Dropbox pack %s" % w["dropbox_key"], "aliases": [], "sources": []}
            packs.append(match)
        match["aliases"].append(w["dropbox_key"])
        match["sources"].insert(0, entry)
    return {
        "source": {
            "archive_commit": archive_commit(archive),
            "bundle": JPD_BUNDLE,
            "bundle_commit": head,
            "note": "Generated by scripts/import_content.py; do not edit by hand. "
                    "Packs are community works; none is copied into this repository.",
        },
        "packs": packs,
    }


# Dropbox key -> the fan-repo pack it is the original of, as established by
# vertix-research assets-research/out/a01-extra-packs.json.
CAPTURE_OF = {
    "13xlc5n3ipudqsn": "sonic-mod-primary",
    "e3lib880qd7rjcl": "nuclear-throne-mod",
}


def dump(obj: dict) -> str:
    return json.dumps(obj, indent=1, ensure_ascii=False) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--archive", required=True, type=Path)
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()
    sums = load_sums(args.archive)
    outputs = {
        OUT / "cosmetics.json": dump(build_cosmetics(args.archive, sums)),
        OUT / "mods.json": dump(build_mods(args.archive, sums)),
    }
    stale = [p for p, text in outputs.items() if not p.is_file() or p.read_text(encoding="utf-8") != text]
    if args.check:
        for p in stale:
            print("stale: %s" % p.relative_to(ROOT))
        return 1 if stale else 0
    OUT.mkdir(parents=True, exist_ok=True)
    for p, text in outputs.items():
        p.write_text(text, encoding="utf-8")
        print("wrote %s" % p.relative_to(ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())

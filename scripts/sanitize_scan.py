#!/usr/bin/env python3
"""Pre-publish sanitization scan for machine- and person-identifying data.

gitleaks (scripts/deep-scan.sh) finds credentials. This finds what gitleaks
does not: e-mail addresses, absolute local paths (POSIX home dirs and any
Windows drive path), private network addresses, personal identifiers from a
local denylist, and original game files that must never be committed.

It checks three places, because all three become public on the flip:
  * every tracked file in the working tree           (--tree, default)
  * every blob and commit message on every ref        (--history)
  * every author/committer name and e-mail            (--history)

Personal identifiers (real name, machine name, OS user name, private e-mail)
are listed one per line in `.sanitize-denylist` at the repo root. That file is
git-ignored so the identifiers themselves are never committed.

Original-file check: if VERTIX_ARCHIVE points at a local vertix-archive clone,
every tracked file's SHA-256 is compared with the archive manifest; any match
means an original was committed.

Exit status 0 = clean, 1 = findings, 2 = usage error.
"""
from __future__ import annotations

import argparse
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

ALLOWED_EMAIL = re.compile(
    r"(@(users\.noreply\.github\.com|noreply\.github\.com|anthropic\.com|example\.(com|org|net))$"
    r"|^git@github\.com$)",
    re.IGNORECASE,
)
EMAIL = re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
PATTERNS = {
    "posix-home-path": re.compile(r"(/Users/[^/\s]+|/home/[^/\s]+/|/root/)"),
    "windows-drive-path": re.compile(r"\b[A-Za-z]:\\\\?[A-Za-z0-9_. -]"),
    "unc-path": re.compile(r"\\\\[A-Za-z0-9_.-]+\\[A-Za-z0-9_$.-]+"),
    "private-ip": re.compile(
        r"\b(10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}"
        r"|172\.(1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}|169\.254\.\d{1,3}\.\d{1,3})\b"
    ),
    "mac-address": re.compile(r"\b([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b"),
}
ORIGINAL_NAMES = re.compile(
    r"(^|/)(app\.js|index\.js|res\.zip|[^/]*\.(apk|wav|ogg|mp3|mp4|webm|m4a|png|jpe?g|gif|ttf|woff2?))$",
    re.IGNORECASE,
)
# Files that define the patterns above necessarily contain them.
SELF = {"scripts/sanitize_scan.py", "scripts/tests/test_sanitize_scan.py", ".pre-commit-config.yaml",
        ".githooks/pre-push"}


def git(*args: str, data: bytes | None = None) -> bytes:
    return subprocess.run(["git", *args], input=data, capture_output=True, check=True).stdout


def load_denylist(root: Path) -> list[str]:
    p = root / ".sanitize-denylist"
    if not p.exists():
        return []
    return [ln.strip() for ln in p.read_text(encoding="utf-8").splitlines()
            if ln.strip() and not ln.lstrip().startswith("#")]


def denied(term: str, text: str) -> bool:
    """Case-insensitive match of a denylist term as a whole word (so a name inside a login does not match)."""
    return re.search(r"(?<![A-Za-z0-9])%s(?![A-Za-z0-9])" % re.escape(term), text, re.IGNORECASE) is not None


def scan_text(text: str, denylist: list[str]) -> list[tuple[str, str]]:
    """Return (kind, matched text) for every finding in one text."""
    found = []
    for m in EMAIL.finditer(text):
        if not ALLOWED_EMAIL.search(m.group(0)):
            found.append(("email", m.group(0)))
    for kind, rx in PATTERNS.items():
        found.extend((kind, m.group(0)) for m in rx.finditer(text))
    found.extend(("denylist", d) for d in denylist if denied(d, text))
    return found


def archive_hashes() -> set[str]:
    root = os.environ.get("VERTIX_ARCHIVE")
    if not root:
        return set()
    sums = Path(root) / "vertix-preservation" / "manifests" / "sha256sums.txt"
    if not sums.exists():
        print("note: VERTIX_ARCHIVE set but %s not found; original-hash check skipped" % sums)
        return set()
    return {ln.split()[0].lower() for ln in sums.read_text(encoding="utf-8").splitlines() if ln.strip()}


def scan_tree(denylist: list[str]) -> list[str]:
    out = []
    originals = archive_hashes()
    for rel in git("ls-files", "-z").decode("utf-8").split("\0"):
        if not rel:
            continue
        if ORIGINAL_NAMES.search(rel):
            out.append("%s: original-game-file name" % rel)
        p = Path(rel)
        if not p.is_file():
            continue
        body = p.read_bytes()
        if originals and body and hashlib.sha256(body).hexdigest() in originals:
            out.append("%s: identical to an archive original" % rel)
        if rel in SELF or b"\0" in body[:8000]:
            continue
        for kind, hit in scan_text(body.decode("utf-8", "replace"), denylist):
            out.append("%s: %s %r" % (rel, kind, hit))
    return out


def scan_history(denylist: list[str]) -> list[str]:
    out = []
    for line in git("log", "--all", "--format=%H%x00%an%x00%ae%x00%cn%x00%ce").decode("utf-8").splitlines():
        sha, an, ae, cn, ce = line.split("\0")
        for who, email in ((an, ae), (cn, ce)):
            if not ALLOWED_EMAIL.search(email):
                out.append("commit %s: author/committer e-mail %r" % (sha[:12], email))
            out.extend("commit %s: name %r matches denylist" % (sha[:12], who)
                       for d in denylist if denied(d, who))
        msg = git("log", "-1", "--format=%B", sha).decode("utf-8", "replace")
        out.extend("commit %s message: %s %r" % (sha[:12], k, h) for k, h in scan_text(msg, denylist))
    # every blob reachable from any ref, scanned once
    seen = set()
    for line in git("rev-list", "--all", "--objects").decode("utf-8").splitlines():
        sha, _, path = line.partition(" ")
        if not path or sha in seen:
            continue
        seen.add(sha)
        if git("cat-file", "-t", sha).strip() != b"blob":
            continue
        if ORIGINAL_NAMES.search(path):
            out.append("history %s (%s): original-game-file name" % (path, sha[:12]))
        if path in SELF:
            continue
        body = git("cat-file", "blob", sha)
        if b"\0" in body[:8000]:
            continue
        out.extend("history %s (%s): %s %r" % (path, sha[:12], k, h)
                   for k, h in scan_text(body.decode("utf-8", "replace"), denylist))
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--history", action="store_true", help="also scan every ref's blobs, messages and authors")
    args = ap.parse_args(argv)
    root = Path(git("rev-parse", "--show-toplevel").decode().strip())
    os.chdir(root)
    denylist = load_denylist(root)
    if not denylist:
        print("note: no .sanitize-denylist; personal-identifier check covers patterns only")
    findings = scan_tree(denylist)
    if args.history:
        findings += scan_history(denylist)
    for f in sorted(set(findings)):
        print(f)
    if findings:
        print("sanitize scan: %d finding(s)" % len(set(findings)))
        return 1
    print("sanitize scan: clean (%s)" % ("tree + history" if args.history else "tree"))
    return 0


if __name__ == "__main__":
    sys.exit(main())

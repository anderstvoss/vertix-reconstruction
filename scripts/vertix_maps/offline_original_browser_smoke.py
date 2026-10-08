#!/usr/bin/env python3
"""Offline browser smoke for UNMODIFIED August-06 Vertix app.js and archived assets.

This is NOT a game-server or successful gameSetup/round test. The browser is
restricted to loopback; all external URLs from the captured page are rewritten
or blocked so historic production servers/third parties cannot be contacted.
Only a temporary page shell is adapted, never the original archived app.js.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from threading import Thread
from urllib.parse import urlsplit

APP = ("vertix-preservation/originals/wayback/20160806061006/"
       "vertix.io/js/app.js.FEMVAY3YIDEPBGEXH32YHQNXPFQDB4N3")
HTML = ("vertix-preservation/originals/wayback/20160807195546/"
        "vertix.io_80/root.I35QF3OGUSBRICREPIY4SD6V657FSLI2")
CSS = ("vertix-preservation/originals/wayback/20160806060840/"
       "vertix.io/css/main.css.A4QUY4KXDH2OCIKET5YQZ32UWNEI4RVB")
APK_WWW = "vertix-preservation/derived/android-0.0.3/assets/www"
JQUERY = "vertix-preservation/originals/external/jquery-2.1.4.min.js"
SOCKET_IO = "vertix-preservation/originals/external/socket.io-1.4.5.js"
EXPECTED = {
    "app_js": "cbab5cd590ff0d3a9d01a60eba835cd885b715988d0d87a7d287321399df2f09",
    "html": "1d1b3be40ad8567c17eddb6d884e28c67685f4c7191cdebbb5a681c629c02217",
    "main_css": "b911b6bb54f8f6a77244dd950d63ae88ab8d921aca219825953f8b556052a590",
    "jquery": "f16ab224bb962910558715c82f58c10c3ed20f153ddfaa199029f141b5b0255c",
    "socket_io": "9702309dfcdbb90b3ac680b42f37089032793f0978704495a0da53448c9059f9",
}


def checked_bytes(base: Path, rel: str, expected: str) -> bytes:
    data = (base / rel).read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        raise ValueError(f"Archived input SHA256 mismatch: {rel}, got {actual}")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive_root", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("--browser", help="Chromium/Chrome executable")
    parser.add_argument("--virtual-time-ms", type=int, default=10000)
    args = parser.parse_args()
    root = args.archive_root.resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    bpath = args.browser or shutil.which("google-chrome") or shutil.which("chromium") or shutil.which("chromium-browser")
    if not bpath:
        raise RuntimeError("No Chrome/Chromium executable available")
    files = {
        "app_js": checked_bytes(root, APP, EXPECTED["app_js"]),
        "html": checked_bytes(root, HTML, EXPECTED["html"]),
        "main_css": checked_bytes(root, CSS, EXPECTED["main_css"]),
        "jquery": checked_bytes(root, JQUERY, EXPECTED["jquery"]),
        "socket_io": checked_bytes(root, SOCKET_IO, EXPECTED["socket_io"]),
    }
    image_paths = []
    with tempfile.TemporaryDirectory(prefix="vertix-browser-smoke-") as scratch:
        static = Path(scratch) / "www"
        apk = root / APK_WWW
        if not apk.is_dir():
            raise FileNotFoundError(f"Expected unpacked original APK WWW assets: {apk}")
        shutil.copytree(apk, static)
        for directory in ("js", "css", "mirror"):
            (static / directory).mkdir(parents=True, exist_ok=True)
        (static / "js/app.js").write_bytes(files["app_js"])
        (static / "css/main.css").write_bytes(files["main_css"])
        (static / "mirror/jquery-2.1.4.min.js").write_bytes(files["jquery"])
        (static / "mirror/socket.io-1.4.5.js").write_bytes(files["socket_io"])
        html = files["html"].decode("utf-8")
        html = html.replace(
            "http://code.jquery.com/jquery-2.1.4.min.js", "/mirror/jquery-2.1.4.min.js"
        ).replace(
            "http://cdn.socket.io/socket.io-1.4.5.js", "/mirror/socket.io-1.4.5.js"
        )
        # The advertising script is third-party and not needed to test gameplay.
        html = html.replace(
            "http://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            "data:text/javascript,"
        )
        (static / "index.html").write_text(html, encoding="utf-8")
        requests = []
        class Handler(SimpleHTTPRequestHandler):
            def __init__(self, *a, **kw):
                super().__init__(*a, directory=str(static), **kw)
            def do_GET(self):
                if urlsplit(self.path).path == "/getIP":
                    # Loopback fixture. This is NOT a Socket.IO server.
                    payload = json.dumps({"ip": "127.0.0.1", "port": self.server.server_port}).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                return super().do_GET()
            def log_message(self, fmt, *a):
                requests.append((self.path, fmt % a))

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            url = f"http://127.0.0.1:{server.server_port}/"
            common = [
                bpath, "--headless=new", "--no-sandbox", "--disable-dev-shm-usage",
                "--disable-gpu", "--disable-background-networking", "--disable-extensions",
                "--no-first-run", "--hide-scrollbars",
                "--host-resolver-rules=MAP * ~NOTFOUND,EXCLUDE localhost,EXCLUDE 127.0.0.1",
                f"--virtual-time-budget={args.virtual_time_ms}",
                "--window-size=1280,800",
            ]
            screenshot = output / "original-client-smoke.png"
            # Independent browser starts because Chrome --dump-dom and
            # --screenshot are handled inconsistently together.
            screen = subprocess.run(
                [*common, f"--screenshot={screenshot}", url],
                capture_output=True, text=True, timeout=65
            )
            dumped = subprocess.run(
                [*common, "--dump-dom", url],
                capture_output=True, text=True, timeout=65
            )
            (output / "chromium-screenshot-stderr.txt").write_text(screen.stderr[-30000:])
            (output / "chromium-dom-stderr.txt").write_text(dumped.stderr[-30000:])
            (output / "dom.html").write_text(dumped.stdout)
        finally:
            server.shutdown()
            server.server_close()
        statuses = Counter()
        for _, log in requests:
            found = re.search(r'" (\\d{3}) ', log)
            statuses[found.group(1) if found else "unknown"] += 1
        result = {
            "build": "2016-08-06",
            "browser_executable": bpath,
            "original_app_unmodified_sha256": EXPECTED["app_js"],
            "original_page_capture": "2016-08-07 adapted local resource routes",
            "client_source_byte_integrity": True,
            "network_policy": "loopback only; external DNS blocked",
            "real_socketio_server_present": False,
            "gameSetup_received": False,
            "real_multiplayer_round_executed": False,
            "screenshot_exit": screen.returncode,
            "dom_exit": dumped.returncode,
            "screenshot_bytes": screenshot.stat().st_size if screenshot.is_file() else 0,
            "dom_bytes": len(dumped.stdout.encode()),
            "request_status_counts": dict(statuses),
            "requested_paths": [req[0] for req in requests][:200],
        }
        (output / "report.json").write_text(json.dumps(result, indent=2)+"\n")
        print(json.dumps(result))
        if screen.returncode or dumped.returncode or result["screenshot_bytes"] == 0:
            raise SystemExit("Chrome failed to render the archived-page shell; inspect logs")


if __name__ == "__main__":
    main()

"""Boot the unmodified August 2016 browser client from a hashed archive.

Only the historical page shell is rewritten in memory to use local archived
CDNs and load our new input shim. Original game files are never checked in or
modified. This is a static bootstrap server, not a gameplay/socket server.

    python3 web/serve.py --archive ../vertix-archive --port 8000 --socket-port 8001
"""
from __future__ import annotations

import argparse
import hashlib
import json
import mimetypes
import re
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = ROOT / "config" / "boot-20160806.json"
CDN = {
    "/__cdn/jquery-2.1.4.min.js": "code.jquery.com/jquery-2.1.4.min.js",
    "/__cdn/socket.io-1.4.5.js": "cdn.socket.io/socket.io-1.4.5.js",
}


class ArchiveAssets:
    def __init__(self, archive: Path, manifest: Path = DEFAULT_MANIFEST):
        self.archive = archive.resolve()
        self.manifest = json.loads(manifest.read_text(encoding="utf-8"))
        self.assets = self.manifest["assets"]

    def read_pinned(self, pick: dict) -> bytes:
        location = pick["path"]
        archive_path, separator, member = location.partition("::")
        file = (self.archive / archive_path).resolve()
        if file != self.archive and self.archive not in file.parents:
            raise ValueError("archive lookup escapes configured root")
        if separator:
            # The original Android APK is a ZIP, not a game-server binary.
            with zipfile.ZipFile(file) as apk:
                body = apk.read(member)
        else:
            body = file.read_bytes()
        actual = hashlib.sha256(body).hexdigest()
        if actual != pick["sha256"]:
            raise ValueError(f"archive hash mismatch: {archive_path}")
        return body

    def get(self, path: str) -> bytes | None:
        lookup = CDN.get(path, path.lstrip("/"))
        if lookup == "js/app.js":
            # Resolves to the exact Wayback byte sequence in the manifest.
            pass
        pick = self.assets.get(lookup)
        return self.read_pinned(pick) if pick else None

    def page(self) -> bytes:
        html = self.read_pinned(self.manifest["page_shell"]).decode("utf-8")
        replacements = {
            "http://code.jquery.com/jquery-2.1.4.min.js":
                "/__cdn/jquery-2.1.4.min.js",
            "http://cdn.socket.io/socket.io-1.4.5.js":
                "/__cdn/socket.io-1.4.5.js",
        }
        for original, local in replacements.items():
            if original not in html:
                raise ValueError(f"page missing expected archived dependency: {original}")
            html = html.replace(original, local)
        # Avoid connecting to archived external advertising infrastructure.
        html = re.sub(
            r'<script\\s+async\\s+src="http://pagead2\\.googlesyndication\\.com/[^"]+"\\s*></script>',
            "", html,
        )
        # The original client registers key handlers when app.js executes.
        # This injection must follow it to reconcile key state after its handler.
        tag = '<script src="js/app.js"></script>'
        if html.count(tag) != 1:
            raise ValueError("expected exactly one game client script tag")
        html = html.replace(
            tag, tag + '\\n<script src="/__compat/input-opposites.js"></script>',
        )
        return html.encode("utf-8")


class Handler(BaseHTTPRequestHandler):
    assets: ArchiveAssets
    socket_port = 8001

    def do_GET(self):
        self.serve_resource()

    def do_HEAD(self):
        # zip.js checks HEAD /res.zip Content-Length before the GET.
        self.serve_resource()

    def serve_resource(self):
        path = urlsplit(self.path).path
        try:
            if path in {"/", "/index.html"}:
                body, content_type = self.assets.page(), "text/html; charset=utf-8"
            elif path == "/getIP":
                body = json.dumps({
                    "ip": "127.0.0.1", "port": self.socket_port, "region": "local"
                }).encode("utf-8")
                content_type = "application/json"
            elif path == "/__compat/input-opposites.js":
                body = (ROOT / "web" / "input-opposites.js").read_bytes()
                content_type = "application/javascript"
            elif path == "/__health":
                body = json.dumps({
                    "build": self.assets.manifest["target_build"],
                    "assets": len(self.assets.assets),
                    "socket_port": self.socket_port,
                    "gameplay_connected": False,
                }).encode("utf-8")
                content_type = "application/json"
            else:
                body = self.assets.get(path)
                content_type = mimetypes.guess_type(path)[0] or "application/octet-stream"
        except (OSError, ValueError, KeyError, zipfile.BadZipFile) as exc:
            self.send_error(500, f"archive integrity / configuration error: {type(exc).__name__}")
            return

        if body is None:
            self.send_error(404, "not in pinned 2016-08-06 asset set")
            return
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def log_message(self, fmt, *args):
        # Intentional minimal output; do not log headers/cookies or private paths.
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--port", type=int, default=8000)
    parser.add_argument("--socket-port", type=int, default=8001)
    args = parser.parse_args()
    Handler.assets = ArchiveAssets(args.archive, args.manifest)
    Handler.socket_port = args.socket_port
    # Deliberately loopback-only. No accidental exposure of historical client.
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"Historical client shell: http://127.0.0.1:{args.port}/", flush=True)
    print(f"Socket target (must be started separately): {args.socket_port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.shutdown()


if __name__ == "__main__":
    main()

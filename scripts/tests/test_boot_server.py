"""Offline, synthetic archive tests: never copy original Vertix binaries into git."""
import hashlib
import json
import tempfile
import threading
import unittest
import zipfile
from pathlib import Path
from urllib.request import Request, urlopen
from http.server import ThreadingHTTPServer

from web.serve import ArchiveAssets, Handler


def pin(root, path, content):
    p = root / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(content)
    return {"path": path, "sha256": hashlib.sha256(content).hexdigest()}


class BootTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        shell = (
            b'<html><script src="http://code.jquery.com/jquery-2.1.4.min.js"></script>'
            b'<script async src="http://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js"></script>'
            b'<script src="http://cdn.socket.io/socket.io-1.4.5.js"></script>'
            b'<script src="js/app.js"></script></html>'
        )
        page = pin(self.root, "page.html", shell)
        self.js = b"/* fake test client, not original */"
        js = pin(self.root, "original.js", self.js)
        zip_body = b"mock-resource-content"
        resources = pin(self.root, "resources.bin", zip_body)
        jquery = pin(self.root, "jquery.js", b"/* jquery placeholder */")
        socket = pin(self.root, "sio.js", b"/* socket placeholder */")
        self.manifest = self.root / "manifest.json"
        self.manifest.write_text(json.dumps({
            "target_build": "synthetic",
            "page_shell": page,
            "assets": {
                "js/app.js": js,
                "res.zip": resources,
                "code.jquery.com/jquery-2.1.4.min.js": jquery,
                "cdn.socket.io/socket.io-1.4.5.js": socket,
            }
        }), encoding="utf-8")
        self.assets = ArchiveAssets(self.root, self.manifest)

    def tearDown(self):
        self.temp.cleanup()

    def test_original_js_is_unmodified(self):
        self.assertEqual(self.assets.get("/js/app.js"), self.js)

    def test_page_rewrites_cdns_and_injects_compat_after_original(self):
        html = self.assets.page().decode("utf-8")
        self.assertIn('/__cdn/jquery-2.1.4.min.js', html)
        self.assertIn('/__cdn/socket.io-1.4.5.js', html)
        self.assertNotIn("googlesyndication.com", html)
        self.assertLess(html.index('<script src="js/app.js"></script>'),
                        html.index('<script src="/__compat/input-opposites.js"></script>'))

    def test_hash_check_fails_closed(self):
        (self.root / "original.js").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            self.assets.get("/js/app.js")

    def test_zip_member_is_pinned_and_verified(self):
        apk_path = self.root / "test.apk"
        with zipfile.ZipFile(apk_path, "w") as archive:
            archive.writestr("assets/www/res.zip", b"zip-inside-apk")
        self.assertEqual(
            self.assets.read_pinned({
                "path": "test.apk::assets/www/res.zip",
                "sha256": hashlib.sha256(b"zip-inside-apk").hexdigest()
            }),
            b"zip-inside-apk"
        )

    def test_http_head_getip_and_compat_shim(self):
        class TestHandler(Handler):
            assets = self.assets
            socket_port = 8765
        server = ThreadingHTTPServer(("127.0.0.1", 0), TestHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f"http://127.0.0.1:{server.server_port}"
            with urlopen(Request(base + "/res.zip", method="HEAD"), timeout=3) as response:
                self.assertEqual(response.status, 200)
                self.assertEqual(response.headers["Content-Length"],
                                 str(len(b"mock-resource-content")))
                self.assertEqual(response.read(), b"")
            with urlopen(base + "/getIP", timeout=3) as response:
                target = json.loads(response.read())
                self.assertEqual(target["port"], 8765)
                self.assertEqual(target["ip"], "127.0.0.1")
            with urlopen(base + "/__compat/input-opposites.js", timeout=3) as response:
                self.assertIn(b"installOppositeKeyFix", response.read())
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=3)


if __name__ == "__main__":
    unittest.main()

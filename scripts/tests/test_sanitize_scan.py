import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import sanitize_scan as s  # noqa: E402


def kinds(text, deny=()):
    return {k for k, _ in s.scan_text(text, list(deny))}


class ScanText(unittest.TestCase):
    def test_personal_email_flagged(self):
        self.assertIn("email", kinds("contact someone@gmail.com"))

    def test_noreply_email_allowed(self):
        self.assertEqual(kinds("Co-Authored-By: x <noreply@anthropic.com> 1+u@users.noreply.github.com"), set())

    def test_paths(self):
        self.assertIn("windows-drive-path", kinds(r"D:\Projects\thing"))
        self.assertIn("posix-home-path", kinds("/home/alice/src"))
        self.assertIn("posix-home-path", kinds("/Users/alice/src"))
        self.assertIn("unc-path", kinds(r"\\nas\share"))

    def test_private_ip_but_not_public(self):
        self.assertIn("private-ip", kinds("192.168.1.20"))
        self.assertEqual(kinds("historical server 54.70.6.193:5007"), set())

    def test_denylist_case_insensitive(self):
        self.assertIn("denylist", kinds("built on MYBOX", ["mybox"]))

    def test_denylist_whole_word_only(self):
        self.assertEqual(kinds("github.com/janedoe/repo", ["jane"]), set())
        self.assertIn("denylist", kinds("by Jane, 2026", ["jane"]))

    def test_ssh_remote_not_email(self):
        self.assertEqual(kinds("git@github.com:owner/repo.git"), set())

    def test_original_names(self):
        for p in ["js/app.js", "res.zip", "a/b.apk", "images/x.png", "s.wav"]:
            self.assertTrue(s.ORIGINAL_NAMES.search(p), p)
        self.assertFalse(s.ORIGINAL_NAMES.search("src/lib.rs"))


if __name__ == "__main__":
    unittest.main()

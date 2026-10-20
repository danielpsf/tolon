import base64
import json
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

import release_assets as ra


def minisign(key_bytes):
    body = base64.b64encode(b"Ed" + key_bytes + b"x" * 8).decode()
    return base64.b64encode(f"untrusted comment: t\n{body}\n".encode()).decode()


class LatestJson(unittest.TestCase):
    def dist(self, files):
        d = Path(tempfile.mkdtemp())
        for name in files:
            (d / name).write_text(f"sig-of-{name}\n" if name.endswith(".sig") else "bin")
        return d

    FULL = [
        "Tolon.app.tar.gz", "Tolon.app.tar.gz.sig", "Tolon_0.1.0_universal.dmg",
        "Tolon_0.1.0_x64-setup.exe", "Tolon_0.1.0_x64-setup.exe.sig",
        "Tolon_0.1.0_x64_en-US.msi", "Tolon_0.1.0_x64_en-US.msi.sig",
        "Tolon_0.1.0_amd64.AppImage", "Tolon_0.1.0_amd64.AppImage.sig",
    ]

    def test_maps_every_platform_and_prefers_nsis(self):
        m = ra.latest_json(self.dist(self.FULL), "v0.1.0", "o/r", datetime(2026, 1, 2, 3, 4, 5, tzinfo=timezone.utc))
        self.assertEqual(m["version"], "0.1.0")
        self.assertEqual(m["pub_date"], "2026-01-02T03:04:05Z")
        self.assertEqual(sorted(m["platforms"]), ["darwin-aarch64", "darwin-x86_64", "linux-x86_64", "windows-x86_64"])
        self.assertEqual(m["platforms"]["windows-x86_64"]["signature"], "sig-of-Tolon_0.1.0_x64-setup.exe.sig")
        self.assertEqual(m["platforms"]["linux-x86_64"]["url"], "https://github.com/o/r/releases/download/v0.1.0/Tolon_0.1.0_amd64.AppImage")

    def test_missing_signature_is_an_error(self):
        files = [f for f in self.FULL if "AppImage.sig" not in f]
        with self.assertRaises(SystemExit) as e:
            ra.latest_json(self.dist(files), "v0.1.0", "o/r")
        self.assertIn("linux-x86_64", str(e.exception))

    def test_written_file_is_valid_json(self):
        d = self.dist(self.FULL)
        ra.latest_json(d, "v0.1.0", "o/r")
        self.assertIn("platforms", json.loads((d / "latest.json").read_text()))


class KeyId(unittest.TestCase):
    def test_key_id_is_bytes_two_to_ten(self):
        self.assertEqual(ra.key_id(minisign(b"ABCDEFGH")), b"ABCDEFGH")


class Tag(unittest.TestCase):
    def test_bad_tags(self):
        self.assertTrue(ra.check_tag("0.1.0"))
        self.assertTrue(any("version" in e for e in ra.check_tag("v9.9.9")))


if __name__ == "__main__":
    unittest.main()

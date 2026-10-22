import base64
import json
import tempfile
import unittest
from unittest import mock
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
        "Tolon_0.1.0_amd64.deb", "Tolon_0.1.0_amd64.deb.sig",
    ]

    def test_maps_every_platform_key_and_points_plain_windows_at_nsis(self):
        m = ra.latest_json(self.dist(self.FULL), "v0.1.0", "o/r", datetime(2026, 1, 2, 3, 4, 5, tzinfo=timezone.utc))
        self.assertEqual(m["version"], "0.1.0")
        self.assertEqual(m["pub_date"], "2026-01-02T03:04:05Z")
        p = m["platforms"]
        self.assertEqual(sorted(p), sorted([
            "darwin-aarch64", "darwin-x86_64", "windows-x86_64", "windows-x86_64-nsis", "windows-x86_64-msi",
            "linux-x86_64", "linux-x86_64-appimage", "linux-x86_64-deb",
        ]))
        self.assertEqual(p["windows-x86_64"], p["windows-x86_64-nsis"])
        self.assertEqual(p["windows-x86_64"]["signature"], "sig-of-Tolon_0.1.0_x64-setup.exe.sig")
        self.assertEqual(p["windows-x86_64-msi"]["signature"], "sig-of-Tolon_0.1.0_x64_en-US.msi.sig")
        self.assertTrue(p["linux-x86_64-deb"]["url"].endswith("/Tolon_0.1.0_amd64.deb"))
        self.assertEqual(m["platforms"]["linux-x86_64"]["url"], "https://github.com/o/r/releases/download/v0.1.0/Tolon_0.1.0_amd64.AppImage")

    NO_MSI = [f for f in FULL if ".msi" not in f]

    def test_prerelease_without_an_msi_leaves_the_msi_key_out(self):
        m = ra.latest_json(self.dist(self.NO_MSI), "v0.1.0-beta.1", "o/r")
        p = m["platforms"]
        self.assertNotIn("windows-x86_64-msi", p)
        self.assertEqual(p["windows-x86_64"], p["windows-x86_64-nsis"])
        self.assertTrue(p["windows-x86_64"]["url"].endswith("/Tolon_0.1.0_x64-setup.exe"))
        self.assertEqual(len(p), 7)

    def test_plain_version_without_an_msi_is_an_error(self):
        with self.assertRaises(SystemExit) as e:
            ra.latest_json(self.dist(self.NO_MSI), "v0.1.0", "o/r")
        self.assertIn("windows-x86_64-msi", str(e.exception))

    def test_prerelease_still_needs_every_other_signed_artifact(self):
        files = [f for f in self.NO_MSI if "setup.exe.sig" not in f]
        with self.assertRaises(SystemExit) as e:
            ra.latest_json(self.dist(files), "v0.1.0-beta.1", "o/r")
        self.assertIn("windows-x86_64", str(e.exception))

    def test_missing_signature_is_an_error(self):
        files = [f for f in self.FULL if "msi.sig" not in f]
        with self.assertRaises(SystemExit) as e:
            ra.latest_json(self.dist(files), "v0.1.0", "o/r")
        self.assertIn("windows-x86_64-msi", str(e.exception))

    def test_older_archives_are_never_offered(self):
        # The app's updater has no zip support (decision "Updater (L-019)"): a signed archive alone is an error.
        old = {"Tolon_0.1.0_x64-setup.exe": "Tolon_0.1.0_x64-setup.nsis.zip",
               "Tolon_0.1.0_x64_en-US.msi": "Tolon_0.1.0_x64_en-US.msi.zip",
               "Tolon_0.1.0_amd64.AppImage": "Tolon_0.1.0_amd64.AppImage.tar.gz"}
        for installer, archive in old.items():
            files = [f.replace(installer, archive) for f in self.FULL]
            with self.assertRaises(SystemExit, msg=archive):
                ra.latest_json(self.dist(files), "v0.1.0", "o/r")

    def test_written_file_is_valid_json(self):
        d = self.dist(self.FULL)
        ra.latest_json(d, "v0.1.0", "o/r")
        self.assertIn("platforms", json.loads((d / "latest.json").read_text()))


class KeyId(unittest.TestCase):
    def test_key_id_is_bytes_two_to_ten(self):
        self.assertEqual(ra.key_id(minisign(b"ABCDEFGH")), b"ABCDEFGH")


class Tag(unittest.TestCase):
    def test_prerelease_is_a_dash_in_the_version(self):
        self.assertTrue(ra.is_prerelease("v0.1.0-beta.1"))
        self.assertFalse(ra.is_prerelease("v0.1.0"))

    def test_bad_tags(self):
        self.assertTrue(ra.check_tag("0.1.0"))
        self.assertTrue(any("version" in e for e in ra.check_tag("v9.9.9")))


class StampInstall(unittest.TestCase):
    SRC = 'tolon_install() {\n  version="@TOLON_VERSION@"\n  case "$version" in "") ;; @*) ;; esac\n}\n'

    def files(self, text):
        d = Path(tempfile.mkdtemp())
        (d / "install.sh").write_text(text)
        return d / "install.sh", d / "out.sh"

    def test_the_tag_version_replaces_the_placeholder_and_nothing_else(self):
        src, out = self.files(self.SRC)
        self.assertEqual(ra.stamp_install(src, out, "v0.1.0-beta.2"), [])
        self.assertEqual(out.read_text(), self.SRC.replace("@TOLON_VERSION@", "0.1.0-beta.2"))

    def test_a_missing_stamp_is_an_error_and_writes_nothing(self):
        src, out = self.files('version="0.0.1"\n')
        errors = ra.stamp_install(src, out, "v0.1.0")
        self.assertEqual(len(errors), 1)
        self.assertIn("@TOLON_VERSION@", errors[0])
        self.assertFalse(out.exists())

    def test_two_stamps_are_an_error(self):
        src, out = self.files(self.SRC + self.SRC)
        self.assertEqual(len(ra.stamp_install(src, out, "v0.1.0")), 1)
        self.assertFalse(out.exists())

    def test_a_bad_tag_is_an_error(self):
        src, out = self.files(self.SRC)
        self.assertEqual(len(ra.stamp_install(src, out, '0.1.0"; rm -rf ~; "')), 1)
        self.assertFalse(out.exists())

    def test_the_repository_copy_holds_exactly_one_stamp(self):
        self.assertEqual(len(ra.INSTALL_STAMP.findall(ra.INSTALL_SH.read_text())), 1)

    def test_check_tag_reports_a_missing_stamp(self):
        _, bare = self.files("tolon_install() { :; }\n")
        with mock.patch.object(ra, "INSTALL_SH", bare.parent / "install.sh"):
            self.assertTrue(any("install.sh" in e for e in ra.check_tag("v9.9.9")))


class ReleaseWorkflow(unittest.TestCase):
    def test_install_sh_is_stamped_before_the_checksums_are_written_and_is_uploaded(self):
        text = (ra.ROOT / ".github/workflows/release.yml").read_text()
        stamp = text.index("release_assets.py stamp-install install.sh dist/install.sh ")
        sums = text.index("name: Write SHA256SUMS")
        create = text.index("gh release create")
        self.assertLess(stamp, sums)
        self.assertLess(sums, create)
        # `dist/*` is every file in dist, so dist/install.sh and SHA256SUMS go up with the installers.
        self.assertIn("dist/*", text[create:].splitlines()[0])


class Notes(unittest.TestCase):
    B = "https://github.com/o/r/blob/v1.0.0/"

    def fix(self, text):
        return ra.absolute_links(text, "o/r", "v1.0.0")

    def test_relative_links_resolve_against_docs(self):
        self.assertEqual(self.fix("[a](help/x.md)"), f"[a]({self.B}docs/help/x.md)")
        self.assertEqual(self.fix("[a](x.md#frag)"), f"[a]({self.B}docs/x.md#frag)")

    def test_parent_link_leaves_docs(self):
        self.assertEqual(self.fix("[a](../README.md)"), f"[a]({self.B}README.md)")

    def test_images_use_raw(self):
        self.assertEqual(
            self.fix("![a](images/x.png) [b](images/x.png)"),
            "![a](https://github.com/o/r/raw/v1.0.0/docs/images/x.png) "
            f"[b]({self.B}docs/images/x.png)",
        )

    def test_path_escaping_the_repo_is_left_alone(self):
        self.assertEqual(self.fix("[a](../../etc/x.md)"), "[a](../../etc/x.md)")

    def test_absolute_and_anchor_links_stay(self):
        for link in ("https://x.io/a", "http://x.io", "mailto:a@b.c", "#know", "/abs/path"):
            self.assertEqual(self.fix(f"[a]({link})"), f"[a]({link})")

    def test_several_links_and_plain_text(self):
        out = self.fix("see [a](a.md) and [b](https://x.io) (not a link)")
        self.assertEqual(out, f"see [a]({self.B}docs/a.md) and [b](https://x.io) (not a link)")

    def test_notes_file_is_head_rule_install(self):
        install = Path(tempfile.mkdtemp()) / "install.md"
        install.write_text("# Install\n[h](help/x.md)\n")
        out = install.parent / "notes.md"
        root = install.parent / "repo"
        (root / "docs/releases").mkdir(parents=True)
        (root / "docs/releases/v1.0.0-test.md").write_text("# Head\n")
        with mock.patch.object(ra, "ROOT", root):
            self.assertEqual(ra.notes("v1.0.0-test", install, out, "o/r"), [])
        self.assertEqual(
            out.read_text(),
            "# Head\n\n---\n\n# Install\n[h](https://github.com/o/r/blob/v1.0.0-test/docs/help/x.md)\n",
        )

    def test_missing_head_is_an_error(self):
        errors = ra.notes("v0.0.0-nope", "unused", "unused", "o/r")
        self.assertEqual(len(errors), 1)
        self.assertIn("docs/releases/v0.0.0-nope.md", errors[0])


if __name__ == "__main__":
    unittest.main()

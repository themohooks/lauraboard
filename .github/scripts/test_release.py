import hashlib
import importlib.util
from html.parser import HTMLParser
from pathlib import Path
import tarfile
import tempfile
import unittest
from urllib.parse import parse_qs, unquote, urlsplit
from unittest import mock
import zipfile

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_download_table_links_to_every_asset_in_the_selected_release(self):
        class Buttons(HTMLParser):
            def __init__(self):
                super().__init__()
                self.links = []
                self.images = []

            def handle_starttag(self, tag, attrs):
                attrs = dict(attrs)
                if tag == "a":
                    self.links.append(attrs["href"])
                elif tag == "img":
                    self.images.append(attrs)

        for version in ("1.2.3", "1.2.3-rc.1"):
            with self.subTest(version=version):
                text = release.download_table("example/lauraboard", version)
                buttons = Buttons()
                buttons.feed(text)
                self.assertEqual(len(buttons.links), 18)
                self.assertEqual(len(set(buttons.links)), 18)
                base = f"https://github.com/example/lauraboard/releases/download/v{version}/"
                self.assertTrue(all(url.startswith(base) for url in buttons.links))
                self.assertEqual({unquote(url.removeprefix(base)) for url in buttons.links}, release.expected_assets(version))
                self.assertEqual(len(buttons.images), len(buttons.links))
                for image in buttons.images:
                    self.assertTrue(image["alt"])
                    self.assertEqual(urlsplit(image["src"]).hostname, "img.shields.io")
                    self.assertTrue(parse_qs(urlsplit(image["src"]).query)["message"])
                self.assertIn(base + "SHA256SUMS", text)
                self.assertIn("DMG", text)
                self.assertIn("Apple Silicon", text)
                self.assertIn("AppImage", text)

    def test_download_table_uses_the_configured_github_host(self):
        text = release.download_table("team/app", "1.2.3", "https://github.example.com/")
        self.assertIn("https://github.example.com/team/app/releases/download/v1.2.3/", text)
        with self.assertRaises(ValueError):
            release.download_table('bad/repo\"<script>', "1.2.3")

    def test_changelog_lists_every_commit(self):
        entries = [("a" * 40, "Fix <crash>", "Ann"), ("b" * 40, "Add editor", "Bob")]
        text = release.changelog(entries, "team/app", "v1.2.2", "1.2.3")
        self.assertIn("- Fix &lt;crash> ([`aaaaaaa`](https://github.com/team/app/commit/" + "a" * 40 + ")) — Ann", text)
        self.assertLess(text.index("Fix"), text.index("Add editor"))
        self.assertIn("https://github.com/team/app/compare/v1.2.2...v1.2.3", text)
        first = release.changelog([], "team/app", None, "0.1.0")
        self.assertIn("Без изменений", first)
        self.assertNotIn("compare", first)

    def test_commits_include_pull_request_commits(self):
        with tempfile.TemporaryDirectory() as temp, mock.patch.object(release, "ROOT", Path(temp)):
            def git(*args):
                release.run("git", "-c", "user.name=T", "-c", "user.email=t@t", *args, capture_output=True)
            git("init", "-q", "-b", "main")
            git("commit", "-q", "--allow-empty", "-m", "Before")
            git("tag", "v1.0.0")
            git("commit", "-q", "--allow-empty", "-m", "On main")
            git("checkout", "-q", "-b", "pr")
            git("commit", "-q", "--allow-empty", "-m", "Inside the PR")
            git("checkout", "-q", "main")
            git("merge", "-q", "--no-ff", "-m", "Merge pull request #1", "pr")
            self.assertEqual(release.previous_tag("1.1.0"), "v1.0.0")
            self.assertEqual(sorted(s for _, s, _ in release.commits("v1.0.0")), ["Inside the PR", "On main"])

    def test_complete_release_checksums(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            expected = release.expected_assets("1.2.3-rc.1")
            self.assertEqual(len(expected), 18)
            for name in expected:
                (directory / name).write_bytes(name.encode())
            release.finalize(directory, "1.2.3-rc.1")
            for line in (directory / "SHA256SUMS").read_text().splitlines():
                digest, name = line.split("  ")
                self.assertEqual(digest, hashlib.sha256(name.encode()).hexdigest())

    def test_incomplete_and_empty_releases_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            with self.assertRaisesRegex(ValueError, "Incomplete release"):
                release.finalize(directory, "1.2.3")
            for name in release.expected_assets("1.2.3"):
                (directory / name).touch()
            with self.assertRaisesRegex(ValueError, "Empty or invalid"):
                release.finalize(directory, "1.2.3")

    def test_unexpected_asset_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            for name in release.expected_assets("1.2.3") | {"wrong-version.exe"}:
                (directory / name).write_bytes(b"data")
            with self.assertRaisesRegex(ValueError, "unexpected=.*wrong-version"):
                release.finalize(directory, "1.2.3")

    def test_portable_archives_have_a_root_folder_and_executable_mode(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            source = directory / "stage"
            source.mkdir()
            (source / "lauraboard").write_bytes(b"binary")
            (source / "lauraboard").chmod(0o755)
            (source / "ffmpeg").write_bytes(b"ffmpeg")
            (source / "ffprobe").write_bytes(b"ffprobe")
            browser = source / "lauraboard-webview"
            browser.parent.mkdir(parents=True, exist_ok=True)
            browser.write_bytes(b"native webview host")
            browser.chmod(0o755)
            release.archive(source, directory / "portable.zip")
            with zipfile.ZipFile(directory / "portable.zip") as archive:
                self.assertEqual(archive.read("portable/lauraboard"), b"binary")
            release.archive(source, directory / "portable.tar.gz")
            with tarfile.open(directory / "portable.tar.gz") as archive:
                entry = archive.getmember("portable/lauraboard")
                self.assertEqual(entry.mode & 0o111, 0o111)
                for tool in ("ffmpeg", "ffprobe"):
                    self.assertEqual(archive.getmember(f"portable/{tool}").mode, 0o755)
                # Windows chmod cannot set POSIX executable bits.
                if release.os.name != "nt":
                    self.assertEqual(archive.getmember("portable/lauraboard-webview").mode, 0o755)

    def test_server_bundle_copies_both_tools_and_notices(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            bundle = root / "bundle"
            bundle.mkdir()
            suffix = ".exe" if release.os.name == "nt" else ""
            for tool in ("ffmpeg", "ffprobe"):
                (bundle / f"{tool}{suffix}").write_bytes(b"binary")
            notices = bundle / "licenses/ffmpeg"
            notices.mkdir(parents=True)
            (notices / "LICENSE.txt").write_text("upstream notice")
            stage = root / "stage"
            with mock.patch.dict("os.environ", {"LB_FFMPEG_DIR": str(bundle)}), mock.patch.object(release, "run") as run:
                release.bundle_ffmpeg(stage)
            self.assertEqual(run.call_count, 2)
            for tool in ("ffmpeg", "ffprobe"):
                self.assertEqual((stage / f"{tool}{suffix}").read_bytes(), b"binary")
            self.assertEqual((stage / "licenses/ffmpeg/LICENSE.txt").read_text(), "upstream notice")

    def test_current_workspace_versions_agree(self):
        self.assertRegex(release.version(), release.VERSION)

    def test_tag_must_match_manifest_version(self):
        with mock.patch.dict("os.environ", {"RELEASE_TAG": "v99.0.0"}):
            with self.assertRaisesRegex(ValueError, "must equal"):
                release.validate()

    def test_mismatched_crate_version_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "crates/lauraboard").mkdir(parents=True)
            (root / "crates/other").mkdir()
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.3"\n')
            (root / "crates/lauraboard/Cargo.toml").write_text('[package]\nversion = "1.2.3"\n')
            (root / "crates/other/Cargo.toml").write_text('[package]\nversion = "1.2.4"\n')
            with self.assertRaisesRegex(ValueError, "Version mismatch"):
                release.version(root)


if __name__ == "__main__":
    unittest.main()

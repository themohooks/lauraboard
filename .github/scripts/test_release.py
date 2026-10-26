import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_complete_release_checksums(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            expected = release.expected_assets("1.2.3-rc.1")
            self.assertEqual(len(expected), 14)
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
            release.archive(source, directory / "portable.zip")
            with zipfile.ZipFile(directory / "portable.zip") as archive:
                self.assertEqual(archive.read("portable/lauraboard"), b"binary")
            release.archive(source, directory / "portable.tar.gz")
            with tarfile.open(directory / "portable.tar.gz") as archive:
                entry = archive.getmember("portable/lauraboard")
                self.assertEqual(entry.mode & 0o111, 0o111)

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

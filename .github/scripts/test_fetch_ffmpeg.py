import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import tarfile
import unittest
from unittest import mock
import zipfile

spec = importlib.util.spec_from_file_location("fetch_ffmpeg", Path(__file__).with_name("fetch_ffmpeg.py"))
fetch_ffmpeg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fetch_ffmpeg)


class FetchFfmpegTests(unittest.TestCase):
    def fixture(self, digest=None, linux=False):
        archive = io.BytesIO()
        if linux:
            with tarfile.open(fileobj=archive, mode="w:xz") as output:
                for name, contents in [("bin/ffmpeg", b"binary"), ("bin/ffprobe", b"binary"), ("LICENSE.txt", b"upstream notice")]:
                    entry = tarfile.TarInfo(f"upstream/{name}")
                    entry.size = len(contents)
                    output.addfile(entry, io.BytesIO(contents))
        else:
            with zipfile.ZipFile(archive, "w") as output:
                for tool in ("ffmpeg", "ffprobe"):
                    output.writestr(f"upstream/bin/{tool}.exe", b"binary")
                output.writestr("upstream/LICENSE.txt", "upstream notice")
        data = archive.getvalue()
        name = "ffmpeg-n8.1-latest-linuxarm64-gpl-8.1.tar.xz" if linux else "ffmpeg-n8.1-latest-win64-gpl-8.1.zip"
        metadata = {"html_url": "https://example.com/release", "assets": [
            {"name": name, "browser_download_url": "https://example.com/archive"},
            {"name": "checksums.sha256", "browser_download_url": "https://example.com/checksums"},
        ]}
        checksum = digest or hashlib.sha256(data).hexdigest()
        responses = [io.BytesIO(json.dumps(metadata).encode()),
                     io.BytesIO(f"{checksum}  {name}\n".encode()), io.BytesIO(data)]
        return responses, checksum

    def test_checked_tools_and_notices_are_staged(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            responses, checksum = self.fixture()
            environment = {"RELEASE_TARGET": "x86_64-pc-windows-msvc", "GITHUB_ENV": str(root / "env"), "RUNNER_TEMP": str(root)}
            with mock.patch.dict(os.environ, environment), \
                    mock.patch.object(fetch_ffmpeg, "request", side_effect=responses), \
                    mock.patch.object(fetch_ffmpeg.subprocess, "run") as run:
                fetch_ffmpeg.fetch()
            bundle = Path((root / "env").read_text().strip().split("=", 1)[1])
            self.assertEqual(run.call_count, 2)
            self.assertEqual((bundle / "ffprobe.exe").read_bytes(), b"binary")
            self.assertEqual((bundle / "licenses/ffmpeg/LICENSE.txt").read_text(), "upstream notice")
            provenance = json.loads((bundle / "licenses/ffmpeg/provenance.json").read_text())
            self.assertEqual(provenance["sha256"], checksum)
            self.assertIn(f"LB_FFMPEG_DIR={bundle}", (root / "env").read_text())

    def test_linux_arm64_tarball_stages_native_tools(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            responses, _ = self.fixture(linux=True)
            environment = {"RELEASE_TARGET": "aarch64-unknown-linux-gnu", "GITHUB_ENV": str(root / "env"), "RUNNER_TEMP": str(root)}
            with mock.patch.dict(os.environ, environment), \
                    mock.patch.object(fetch_ffmpeg, "request", side_effect=responses), \
                    mock.patch.object(fetch_ffmpeg.subprocess, "run") as run:
                fetch_ffmpeg.fetch()
            bundle = Path((root / "env").read_text().strip().split("=", 1)[1])
            self.assertEqual((bundle / "ffmpeg").read_bytes(), b"binary")
            self.assertEqual((bundle / "ffprobe").read_bytes(), b"binary")
            self.assertEqual(run.call_count, 2)

    def test_checksum_mismatch_prevents_execution_and_export(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            responses, _ = self.fixture("0" * 64)
            environment = {"RELEASE_TARGET": "x86_64-pc-windows-msvc", "GITHUB_ENV": str(root / "env"), "RUNNER_TEMP": str(root)}
            with mock.patch.dict(os.environ, environment), \
                    mock.patch.object(fetch_ffmpeg, "request", side_effect=responses), \
                    mock.patch.object(fetch_ffmpeg.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    fetch_ffmpeg.fetch()
            run.assert_not_called()
            self.assertFalse((root / "env").exists())


if __name__ == "__main__":
    unittest.main()

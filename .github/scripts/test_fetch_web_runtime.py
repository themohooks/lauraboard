import base64
import hashlib
import json
from io import BytesIO
from email.message import Message
from pathlib import Path
import tempfile
import tarfile
import unittest
from unittest.mock import patch

import fetch_web_runtime as runtime


class RuntimeTests(unittest.TestCase):
    def response(self, data):
        response = BytesIO(data)
        response.headers = Message()
        response.headers.add_header("x-goog-hash", "crc32c=unused")
        response.headers.add_header("x-goog-hash", "md5=" + base64.b64encode(hashlib.md5(data).digest()).decode())
        return response

    def test_download_verifies_duplicate_google_hash_headers(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "archive"
            with patch.object(runtime.urllib.request, "urlopen", return_value=self.response(b"runtime")):
                runtime.download("https://example.test/runtime", target)
            self.assertEqual(target.read_bytes(), b"runtime")

    def test_rejects_corrupt_npm_package(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(runtime.urllib.request, "urlopen", return_value=self.response(b"corrupt")):
                with self.assertRaisesRegex(ValueError, "Integrity mismatch"):
                    runtime.download("https://example.test/ruffle", Path(directory) / "archive", "sha512-wrong")

    def test_prepare_ships_only_player_removes_old_browser_and_reuses_cache(self):
        def download(url, destination, integrity=None):
            with tarfile.open(destination, "w:gz") as archive:
                for name in ("ruffle.js", "module.wasm", "LICENSE"):
                    entry = tarfile.TarInfo("package/" + name)
                    entry.size = 4
                    archive.addfile(entry, BytesIO(b"data"))

        metadata = {"dist": {"tarball": "https://example.test/ruffle.tgz", "integrity": "sha512-test"}}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "runtime"
            (output / "browser").mkdir(parents=True)
            (output / "browser/chrome.exe").write_bytes(b"old browser")
            with patch.object(runtime, "download", side_effect=download) as fetch, patch.object(runtime.urllib.request, "urlopen", return_value=BytesIO(json.dumps(metadata).encode())):
                runtime.prepare(output, "windows", "x86_64")
                self.assertEqual(fetch.call_count, 1)
                self.assertEqual(fetch.call_args.args[0], metadata["dist"]["tarball"])
            self.assertFalse((output / "browser").exists())
            self.assertTrue((output / "ruffle/module.wasm").is_file())
            with patch.object(runtime, "download") as fetch:
                runtime.prepare(output, "windows", "x86_64")
                fetch.assert_not_called()


if __name__ == "__main__":
    unittest.main()

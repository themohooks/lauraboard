"""Prepare LauraBoard's offline SWF player (Python 3.11+).

HTML and sign-in use the native system WebView. Only Ruffle and its licenses
are shipped; no Chromium or other browser is downloaded.
"""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import platform
import shutil
import tarfile
import tempfile
import urllib.request

RUFFLE_VERSION = "0.5.0"


def download(url, destination, integrity=None):
    with urllib.request.urlopen(url, timeout=120) as response, destination.open("wb") as output:
        hashes = (v.strip() for header in response.headers.get_all("x-goog-hash", []) for v in header.split(","))
        md5 = next((v[4:] for v in hashes if v.startswith("md5=")), None)
        shutil.copyfileobj(response, output)
    if integrity:
        algorithm, expected = integrity.split("-", 1)
    elif md5:
        algorithm, expected = "md5", md5
    else:
        raise ValueError(f"Missing upstream integrity for {url}")
    with destination.open("rb") as stream:
        actual = base64.b64encode(hashlib.file_digest(stream, algorithm).digest()).decode()
    if actual != expected:
        raise ValueError(f"Integrity mismatch for {url}")


def prepare(output, system, arch):
    version = {"webview": "system", "ruffle": RUFFLE_VERSION}
    marker = output / "versions.json"
    # Remove the old bundled browser even when the Ruffle cache is current.
    browser = output / "browser"
    if browser.is_dir():
        if browser.resolve().parent != output.resolve():
            raise ValueError("Browser cache must remain inside runtime output")
        shutil.rmtree(browser)
    if marker.is_file() and json.loads(marker.read_text()) == version and (output / "ruffle/ruffle.js").is_file():
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="lb-web-", dir=output.parent) as temporary:
        stage = Path(temporary)
        with urllib.request.urlopen(f"https://registry.npmjs.org/@ruffle-rs/ruffle/{RUFFLE_VERSION}", timeout=120) as response:
            metadata = json.load(response)
        player = stage / "ruffle.tgz"
        download(metadata["dist"]["tarball"], player, metadata["dist"]["integrity"])
        with tarfile.open(player) as package:
            package.extractall(stage / "player", filter="data")
        # Replace only runtime directories owned by this script.
        output.mkdir(parents=True, exist_ok=True)
        for name, source in (("ruffle", stage / "player/package"),):
            destination = output / name
            if destination.exists():
                shutil.rmtree(destination)
            shutil.copytree(source, destination, symlinks=True)
        marker.write_text(json.dumps(version, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--system", choices=("windows", "linux", "macos"), default={"Windows": "windows", "Linux": "linux", "Darwin": "macos"}.get(platform.system()))
    parser.add_argument("--arch", choices=("x86", "x86_64", "aarch64"), default={"AMD64": "x86_64", "x86_64": "x86_64", "ARM64": "aarch64", "arm64": "aarch64", "i686": "x86", "x86": "x86"}.get(platform.machine()))
    args = parser.parse_args()
    prepare(args.output.resolve(), args.system, args.arch)


if __name__ == "__main__":
    main()

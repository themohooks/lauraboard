"""Prepare LauraBoard's private browser and offline SWF player (Python 3.11+).

No installed browser or WebView is used. The complete runtime directory is
shipped in installers and portable packages, including licenses and SwiftShader.
"""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import platform
import shutil
import stat
import tarfile
import tempfile
import urllib.request
import zipfile

CHROME_VERSION = "156.0.8078.73"
RUFFLE_VERSION = "0.5.0"


def browser_platform(system, arch):
    if system == "windows":
        # Windows on ARM runs the x64 runtime through its built-in emulation.
        return "win32" if arch == "x86" else "win64"
    if system == "macos":
        return "mac-arm64" if arch == "aarch64" else "mac-x64"
    if system == "linux" and arch in ("x86_64", "aarch64"):
        return "linux-arm64" if arch == "aarch64" else "linux64"
    raise ValueError(f"No supported private web runtime for {system}/{arch}")


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
    selected = browser_platform(system, arch)
    version = {"chrome": CHROME_VERSION, "ruffle": RUFFLE_VERSION, "platform": selected}
    marker = output / "versions.json"
    executable = "chrome.exe" if system == "windows" else "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing" if system == "macos" else "chrome"
    if marker.is_file() and json.loads(marker.read_text()) == version and (output / "browser" / executable).is_file() and (output / "ruffle/ruffle.js").is_file():
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="lb-web-", dir=output.parent) as temporary:
        stage = Path(temporary)
        archive = stage / "browser.zip"
        url = f"https://storage.googleapis.com/chrome-for-testing-public/{CHROME_VERSION}/{selected}/chrome-{selected}.zip"
        download(url, archive)
        with zipfile.ZipFile(archive) as package:
            package.extractall(stage / "chrome")
            if system != "windows":
                for entry in package.infolist():
                    mode = entry.external_attr >> 16
                    path = stage / "chrome" / entry.filename
                    if stat.S_ISLNK(mode):
                        target = package.read(entry).decode("utf-8")
                        if not (path.parent / target).resolve().is_relative_to((stage / "chrome").resolve()):
                            raise ValueError(f"Runtime symlink escapes archive: {entry.filename}")
                        path.unlink()
                        path.symlink_to(target)
                    elif mode:
                        path.chmod(mode & 0o777)
        with urllib.request.urlopen(f"https://registry.npmjs.org/@ruffle-rs/ruffle/{RUFFLE_VERSION}", timeout=120) as response:
            metadata = json.load(response)
        player = stage / "ruffle.tgz"
        download(metadata["dist"]["tarball"], player, metadata["dist"]["integrity"])
        with tarfile.open(player) as package:
            package.extractall(stage / "player", filter="data")
        # Replace only runtime directories owned by this script.
        output.mkdir(parents=True, exist_ok=True)
        for name, source in (("browser", stage / "chrome" / f"chrome-{selected}"), ("ruffle", stage / "player/package")):
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

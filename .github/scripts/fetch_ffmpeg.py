"""Download checked native static FFmpeg tools and retain upstream notices."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
import zipfile

REPOSITORY = "BtbN/FFmpeg-Builds"
TARGETS = {
    "x86_64-pc-windows-msvc": "win64",
    "aarch64-pc-windows-msvc": "winarm64",
    "x86_64-unknown-linux-gnu": "linux64",
    "aarch64-unknown-linux-gnu": "linuxarm64",
}


def request(url, authenticated=False):
    headers = {"User-Agent": "LauraBoard-build"}
    if authenticated and (token := os.environ.get("GH_TOKEN")):
        headers["Authorization"] = f"Bearer {token}"
    return urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=120)


def fetch():
    platform = TARGETS[os.environ["RELEASE_TARGET"]]
    windows = platform.startswith("win")
    extension = ".zip" if windows else ".tar.xz"
    name = f"ffmpeg-n8.1-latest-{platform}-gpl-8.1{extension}"
    with request(f"https://api.github.com/repos/{REPOSITORY}/releases/tags/latest", True) as response:
        release = json.load(response)
    assets = {asset["name"]: asset for asset in release["assets"]}
    asset = assets[name]
    print(f"Preparing {name}", flush=True)
    with request(assets["checksums.sha256"]["browser_download_url"]) as response:
        checksums = response.read().decode()
    expected = next(line.split()[0] for line in checksums.splitlines()
                    if line.split()[-1].lstrip("*") == name)
    # Keep tools outside Cargo's cache, which can restore partial directories.
    bundle = Path(tempfile.mkdtemp(prefix="lauraboard-ffmpeg-bundle-", dir=os.environ.get("RUNNER_TEMP")))
    with tempfile.TemporaryDirectory(prefix="lauraboard-ffmpeg-") as temp:
        archive = Path(temp) / name
        with request(asset["browser_download_url"]) as response, archive.open("wb") as output:
            shutil.copyfileobj(response, output)
        with archive.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if digest != expected:
            raise ValueError(f"FFmpeg checksum mismatch: {digest} != {expected}")
        print(f"Verified SHA256: {digest}", flush=True)
        unpacked = Path(temp) / "unpacked"
        if windows:
            with zipfile.ZipFile(archive) as source:
                source.extractall(unpacked)
        else:
            with tarfile.open(archive) as source:
                source.extractall(unpacked, filter="data")
        source, = unpacked.iterdir()
        for tool in ("ffmpeg", "ffprobe"):
            binary = f"{tool}.exe" if windows else tool
            destination = bundle / binary
            shutil.copy2(source / "bin" / binary, destination)
            destination.chmod(0o755)
            subprocess.run([str(destination), "-version"], check=True)
        notices = bundle / "licenses/ffmpeg"
        shutil.copytree(source, notices, ignore=shutil.ignore_patterns("bin"))
        provenance = {"repository": REPOSITORY, "release": release["html_url"],
                      "asset": asset["browser_download_url"], "sha256": digest}
        (notices / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as output:
        output.write(f"LB_FFMPEG_DIR={bundle}\n")
    print(f"Bundled FFmpeg tools: {bundle}", flush=True)


if __name__ == "__main__":
    fetch()

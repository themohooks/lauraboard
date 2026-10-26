"""Build 32-bit FFmpeg + libx264 on Ubuntu, with checked source archives."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

FFMPEG_URL = "https://ffmpeg.org/releases/ffmpeg-8.1.tar.xz"
FFMPEG_SHA256 = "b072aed6871998cce9b36e7774033105ca29e33632be5b6347f3206898e0756a"
X264_URL = "https://codeload.github.com/mirror/x264/tar.gz/31e19f92f00c7003fa115047ce50978bc98c3a0d"
X264_SHA256 = "d053c9d86988d6bc78237ca5205865c5ddf99c98ef4cd9927eec8f6d388f6dd9"


def source(url, digest, directory):
    archive = directory / "source.tar"
    with urllib.request.urlopen(url, timeout=120) as response, archive.open("wb") as output:
        shutil.copyfileobj(response, output)
    with archive.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != digest:
            raise ValueError(f"Source checksum mismatch: {url}")
    unpacked = directory / "unpacked"
    with tarfile.open(archive) as package:
        package.extractall(unpacked, filter="data")
    return next(unpacked.iterdir())


def build():
    windows = os.environ["FFMPEG_PLATFORM"] == "windows"
    jobs = f"-j{min(os.cpu_count() or 2, 4)}"
    bundle = Path("target/ffmpeg-x86").resolve()
    bundle.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ffmpeg-x86-") as temporary:
        root = Path(temporary)
        prefix = root / "install"
        environment = dict(os.environ, PKG_CONFIG_LIBDIR=str(prefix / "lib/pkgconfig"))

        def run(directory, *args):
            try:
                subprocess.run(args, cwd=directory, env=environment, check=True)
            except subprocess.CalledProcessError:
                for log in (directory / "ffbuild/config.log", directory / "config.log"):
                    if log.is_file():
                        print(log.read_text(errors="replace")[-16000:], flush=True)
                raise

        (root / "x264").mkdir()
        x264 = source(X264_URL, X264_SHA256, root / "x264")
        options = [f"--prefix={prefix}", "--enable-static", "--disable-cli", "--enable-pic"]
        if windows:
            options += ["--host=i686-w64-mingw32", "--cross-prefix=i686-w64-mingw32-"]
        else:
            options += ["--host=i686-linux-gnu", "--extra-cflags=-m32", "--extra-ldflags=-m32"]
            environment["CC"] = "gcc -m32"
        run(x264, "./configure", *options)
        run(x264, "make", jobs)
        run(x264, "make", "install")
        environment.pop("CC", None)
        (root / "ffmpeg").mkdir()
        ffmpeg = source(FFMPEG_URL, FFMPEG_SHA256, root / "ffmpeg")
        options = ["--arch=x86", "--disable-debug", "--disable-doc", "--disable-ffplay",
                   "--disable-autodetect", "--enable-gpl", "--enable-libx264", "--enable-static",
                   "--disable-shared", "--pkg-config=pkg-config", "--pkg-config-flags=--static", f"--extra-cflags=-I{prefix}/include",
                   f"--extra-ldflags=-L{prefix}/lib"]
        if windows:
            options += ["--enable-cross-compile", "--target-os=mingw32", "--cross-prefix=i686-w64-mingw32-",
                        "--extra-ldflags=-static"]
        else:
            options += ["--extra-cflags=-m32", "--extra-ldflags=-m32"]
        run(ffmpeg, "./configure", *options)
        run(ffmpeg, "make", jobs)
        suffix = ".exe" if windows else ""
        for tool in ("ffmpeg", "ffprobe"):
            shutil.copy2(ffmpeg / f"{tool}{suffix}", bundle / f"{tool}{suffix}")
        notices = bundle / "licenses/ffmpeg"
        notices.mkdir(parents=True)
        for name in ("COPYING.GPLv2", "COPYING.GPLv3", "LICENSE.md"):
            shutil.copy2(ffmpeg / name, notices / name)
        shutil.copy2(x264 / "COPYING", notices / "x264-COPYING")
        (notices / "provenance.json").write_text(json.dumps({
            "ffmpeg": {"source": FFMPEG_URL, "sha256": FFMPEG_SHA256},
            "x264": {"source": X264_URL, "sha256": X264_SHA256},
            "configure": options,
        }, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    build()

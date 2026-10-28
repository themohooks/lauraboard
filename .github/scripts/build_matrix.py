"""One native job per target, with all selected formats packaged locally."""

import json
import os


ARCHES = {"x64": "x86_64", "x86": "i686", "arm64": "aarch64"}
FORMATS = {
    "windows": ["setup", "portable", "server"],
    "linux": ["setup", "portable", "server", "appimage", "debpackage", "rpmpackage", "aur"],
}


def matrices(platform="all", architecture="all", package_format="all", release_profile=False):
    if platform not in ("all", *FORMATS):
        raise ValueError(f"Unknown platform: {platform}")
    if architecture not in ("all", *ARCHES):
        raise ValueError(f"Unknown architecture: {architecture}")
    if package_format not in ("all", *set(sum(FORMATS.values(), []))):
        raise ValueError(f"Unknown format: {package_format}")
    builds, packages = [], []
    for system, formats in FORMATS.items():
        if platform not in ("all", system):
            continue
        selected = [f for f in formats if package_format in ("all", f)]
        if release_profile and system == "linux":
            selected = [f for f in selected if f in ("appimage", "debpackage", "rpmpackage")]
        if not selected:
            continue
        for label, arch in ARCHES.items():
            if system == "windows" and label == "x86":
                continue
            if release_profile and system == "linux" and label == "x86":
                continue
            if architecture not in ("all", label):
                continue
            target = f"{arch}-" + ("pc-windows-msvc" if system == "windows" else "unknown-linux-gnu")
            runner = ("windows-11-arm" if label == "arm64" else "windows-2022") if system == "windows" else (
                "ubuntu-24.04-arm" if label == "arm64" else "ubuntu-24.04")
            builds.append({"platform": system, "arch": label, "target": target, "os": runner,
                           "toolchain": f"1.96.0-{target}" if label == "x86" else "1.96.0",
                           "formats": json.dumps(selected)})
            # Packaging uses the matching Linux architecture (including i386 ELF tools).
            packages.extend({"target": target, "arch": label, "platform": system,
                             "os": runner, "format": fmt} for fmt in selected)
    if not builds:
        raise ValueError(f"No packages for {platform}/{architecture}/{package_format}")
    return {"build": {"include": builds}, "package": {"include": packages}}


if __name__ == "__main__":
    result = matrices(os.environ.get("BUILD_PLATFORM", "all"),
                      os.environ.get("BUILD_ARCHITECTURE", "all"),
                      os.environ.get("BUILD_FORMAT", "all"),
                      os.environ.get("BUILD_RELEASE_PROFILE") == "true")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for name, value in result.items():
            output.write(f"{name}={json.dumps(value, separators=(',', ':'))}\n")

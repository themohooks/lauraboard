"""Native release builds and packaging; Python 3.11+, no third-party packages."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "target/release-assets"
TARGETS = {
    "x86_64-pc-windows-msvc": ("windows", "x86_64"),
    "aarch64-pc-windows-msvc": ("windows", "aarch64"),
    "x86_64-unknown-linux-gnu": ("linux", "x86_64"),
    "aarch64-unknown-linux-gnu": ("linux", "aarch64"),
    "x86_64-apple-darwin": ("macos", "x86_64"),
    "aarch64-apple-darwin": ("macos", "aarch64"),
}
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?")


def run(*args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def version(root=ROOT):
    app = tomllib.loads((root / "crates/lauraboard/Cargo.toml").read_text("utf-8"))["package"]["version"]
    if not VERSION.fullmatch(app):
        raise ValueError(f"Unsupported release version: {app}")
    workspace = tomllib.loads((root / "Cargo.toml").read_text("utf-8"))["workspace"]["package"]["version"]
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text("utf-8"))["package"]
        declared = package["version"]
        resolved = workspace if isinstance(declared, dict) and declared.get("workspace") else declared
        if resolved != app:
            raise ValueError(f"Version mismatch in {manifest}: {resolved} != {app}")
    if workspace != app:
        raise ValueError(f"Workspace version {workspace} != app version {app}")
    return app


def validate():
    release_version = version()
    tag = os.environ.get("RELEASE_TAG", "")
    if tag and tag != f"v{release_version}":
        raise ValueError(f"Tag {tag!r} must equal v{release_version}")
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(f"version={release_version}\n")
    print(f"Validated LauraBoard {release_version}", flush=True)


def expected_assets(release_version):
    result = set()
    for target, (system, arch) in TARGETS.items():
        extension = ".zip" if system == "windows" else ".tar.gz"
        prefix = f"lauraboard-{release_version}"
        result.add(f"{prefix}-{target}-server{extension}")
        if system != "macos":
            result.add(f"{prefix}-{target}-desktop{extension}")
            result.add(f"{prefix}-{system}-{arch}-setup" + (".exe" if system == "windows" else ""))
    return result


def archive(source, destination):
    """Keep a containing directory and executable modes inside portable archives."""
    stem = destination.name.removesuffix(".tar.gz").removesuffix(".zip")
    if destination.suffix == ".zip":
        with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as output:
            for path in sorted(source.rglob("*")):
                if path.is_file():
                    output.write(path, str(Path(stem) / path.relative_to(source)))
    else:
        def permissions(entry):
            # Apply POSIX modes explicitly, including when packaging is tested
            # on Windows where chmod cannot set executable bits.
            entry.mode = 0o755 if entry.isdir() or Path(entry.name).name in {"lauraboard", "uninstall", "ffmpeg", "ffprobe"} else 0o644
            entry.uid = entry.gid = 0
            entry.uname = entry.gname = "root"
            return entry

        with tarfile.open(destination, "w:gz") as output:
            output.add(source, arcname=stem, filter=permissions)


def build_info(directory, release_version, target, kind):
    revision = run("git", "rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
    compiler = run("rustc", "--version", capture_output=True, text=True).stdout.strip()
    info = {"name": "LauraBoard", "version": release_version, "target": target,
            "kind": kind, "commit": revision, "compiler": compiler}
    (directory / "build-info.json").write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")


def smoke(binary, release_version):
    output = run(str(binary), "--version", capture_output=True, text=True).stdout.strip()
    if output != f"lauraboard {release_version}":
        raise ValueError(f"Unexpected CLI version: {output!r}")
    run(str(binary), "--help", stdout=subprocess.DEVNULL)
    run(str(binary), "-p", str(ROOT / "examples/demo"), "validate", "--no-media")


def bundle_ffmpeg(stage):
    if bundle := os.environ.get("LB_FFMPEG_DIR"):
        directory = Path(bundle)
        suffix = ".exe" if os.name == "nt" else ""
        for tool in ("ffmpeg", "ffprobe"):
            binary = directory / f"{tool}{suffix}"
            run(str(binary), "-version", stdout=subprocess.DEVNULL)
        shutil.copytree(directory, stage, dirs_exist_ok=True)


def build():
    release_version = version()
    target = os.environ["RELEASE_TARGET"]
    system, arch = TARGETS[target]
    rustc = run("rustc", "-vV", capture_output=True, text=True).stdout
    if f"host: {target}\n" not in rustc:
        raise ValueError(f"Native toolchain required for {target}; got:\n{rustc}")
    # cargo dist packages the host architecture; reject target overrides rather
    # than putting cross-compiled files into a package labelled as the host.
    if os.environ.get("CARGO_BUILD_TARGET") or os.environ.get("CARGO_TARGET_DIR"):
        raise ValueError("Release builds require default Cargo target paths")
    ASSETS.mkdir(parents=True, exist_ok=True)
    if any(ASSETS.iterdir()):
        raise ValueError(f"Release output must be empty: {ASSETS}")
    suffix = ".exe" if system == "windows" else ""
    extension = ".zip" if system == "windows" else ".tar.gz"
    prefix = f"lauraboard-{release_version}"
    if system != "macos":
        run("cargo", "run", "--locked", "-p", "xtask", "--", "dist")
        smoke(ROOT / f"target/release/lauraboard{suffix}", release_version)
        setup = f"{prefix}-{system}-{arch}-setup{suffix}"
        shutil.copy2(ROOT / "target/dist" / setup, ASSETS / setup)
        stage = ROOT / "target/dist/stage"
        shutil.copytree(ROOT / "examples", stage / "examples", dirs_exist_ok=True)
        build_info(stage, release_version, target, "desktop")
        archive(stage, ASSETS / f"{prefix}-{target}-desktop{extension}")

    # A separate target directory keeps the desktop binary and embedded
    # installer payload from being overwritten by the headless build.
    run("cargo", "build", "--release", "--locked", "-p", "lauraboard",
        "--no-default-features", "--target-dir", "target/server")
    binary = ROOT / f"target/server/release/lauraboard{suffix}"
    smoke(binary, release_version)
    # Keep staging outside Cargo's cache, which may restore empty directories
    # after pruning the non-dependency files from a previous build.
    with tempfile.TemporaryDirectory(prefix="lauraboard-server-") as temporary:
        stage = Path(temporary)
        shutil.copy2(binary, stage / binary.name)
        shutil.copy2(ROOT / "README.md", stage / "README.md")
        shutil.copytree(ROOT / "docs", stage / "docs", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        shutil.copytree(ROOT / "examples", stage / "examples")
        bundle_ffmpeg(stage)
        build_info(stage, release_version, target, "server")
        archive(stage, ASSETS / f"{prefix}-{target}-server{extension}")
    print("Release assets:", *(p.name for p in sorted(ASSETS.iterdir())), sep="\n")


def finalize(directory=ASSETS, release_version=None):
    expected = expected_assets(release_version or version())
    actual = {p.name for p in directory.iterdir()}
    if actual != expected:
        raise ValueError(f"Incomplete release: missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}")
    lines = []
    for name in sorted(expected):
        path = directory / name
        if not path.is_file() or not path.stat().st_size:
            raise ValueError(f"Empty or invalid release asset: {path}")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        lines.append(f"{digest}  {name}\n")
    (directory / "SHA256SUMS").write_text("".join(lines), encoding="utf-8", newline="\n")
    print(f"Verified {len(expected)} assets and generated SHA256SUMS")


if __name__ == "__main__":
    commands = {"validate": validate, "build": build, "finalize": finalize}
    if len(sys.argv) != 2 or sys.argv[1] not in commands:
        sys.exit("usage: release.py validate|build|finalize")
    commands[sys.argv[1]]()

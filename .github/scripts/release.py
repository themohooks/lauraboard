"""Native release builds and packaging; Python 3.11+, no third-party packages."""

import hashlib
from html import escape
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
from urllib.parse import quote, urlencode
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "target/release-assets"
TARGETS = {
    "x86_64-pc-windows-msvc": ("windows", "x86_64"),
    "i686-pc-windows-msvc": ("windows", "x86"),
    "aarch64-pc-windows-msvc": ("windows", "aarch64"),
    "x86_64-unknown-linux-gnu": ("linux", "x86_64"),
    "i686-unknown-linux-gnu": ("linux", "x86"),
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
        if system == "linux":
            if arch != "x86":
                for extension in ("AppImage", "deb", "rpm"):
                    result.add(f"{prefix}-{target}.{extension}")
            continue
        result.add(f"{prefix}-{target}-server{extension}")
        if system != "macos":
            result.add(f"{prefix}-{target}-desktop{extension}")
            result.add(f"{prefix}-{system}-{arch}-setup" + (".exe" if system == "windows" else ""))
    return result


def download_table(repository, release_version, server_url="https://github.com"):
    """GitHub-compatible table with one download badge per published asset."""
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError(f"Expected owner/repository, got {repository!r}")
    base = f"{server_url.rstrip('/')}/{repository}/releases/download/{quote('v' + release_version, safe='')}"
    prefix = f"lauraboard-{release_version}"
    linked = set()

    def button(name, label, arch, color, system):
        linked.add(name)
        badge = "https://img.shields.io/static/v1?" + urlencode({
            "label": label, "message": arch, "color": color,
            "labelColor": "454b52", "style": "flat-square",
        })
        return (f'<a href="{escape(base + "/" + quote(name, safe=""), quote=True)}">'
                f'<img src="{escape(badge, quote=True)}" alt="{escape(system + " " + label + " " + arch, quote=True)}"></a>')

    rows = []
    for system, title in (("windows", "Windows"), ("macos", "macOS"), ("linux", "Linux")):
        formats = [("setup", "Setup", "1684c7"), ("desktop", "Portable", "269ca8")]
        if system == "macos":
            formats = []
        elif system == "linux":
            formats = [("AppImage", "AppImage", "e05d2f"), ("deb", "DebPackage", "d36b8c"),
                       ("rpm", "RpmPackage", "d6a22a")]
        if system != "linux":
            formats += [("server", "Server", "747b86")]
        lines = []
        for kind, label, color in formats:
            buttons = []
            for target, (target_system, arch) in TARGETS.items():
                if target_system != system:
                    continue
                if system == "linux" and arch == "x86":
                    continue
                arch_label = {"x86_64": "x64", "x86": "x86", "aarch64": "ARM64"}[arch]
                if system == "macos":
                    arch_label = "Intel x64" if arch == "x86_64" else "Apple Silicon"
                if kind == "setup":
                    name = f"{prefix}-{system}-{arch}-setup" + (".exe" if system == "windows" else "")
                elif kind in ("desktop", "server"):
                    extension = ".zip" if system == "windows" else ".tar.gz"
                    name = f"{prefix}-{target}-{kind}{extension}"
                else:
                    name = f"{prefix}-{target}.{kind}"
                buttons.append(button(name, label, arch_label, color, title))
            lines.append(" ".join(buttons))
        rows.append(f'<tr><td><strong>{title}</strong></td><td>' + "<br>".join(lines) + "</td></tr>")
    if linked != expected_assets(release_version):
        raise ValueError("Download table does not match the release asset set")
    checksum = f"{base}/SHA256SUMS"
    return ("## Скачать для вашей ОС\n\n"
            '<table>\n<tr><th>ОС</th><th>Скачать</th></tr>\n' + "\n".join(rows) + "\n</table>\n\n"
            "**x86** — 32 бита; **ARM64** — 64-битный ARM. "
            "Server — CLI/API без GUI; для macOS доступен только Server.\n\n"
            "FFmpeg и ffprobe устанавливаются отдельно.\n\n"
            f"[SHA256SUMS — контрольные суммы файлов]({checksum})\n\n---\n\n")


def previous_tag(release_version):
    """Newest release tag before this one that is an ancestor of HEAD."""
    tags = run("git", "tag", "--merged", "HEAD", "--sort=-v:refname", "v[0-9]*",
               capture_output=True, text=True).stdout.split()
    return next((t for t in tags if t != f"v{release_version}" and VERSION.fullmatch(t[1:])), None)


def commits(since):
    """`(sha, subject, author)` of every commit since the tag, newest first.
    Commits of merged pull requests are included; the merge commits
    themselves are skipped (they change nothing of their own)."""
    span = f"{since}..HEAD" if since else "HEAD"
    out = run("git", "log", "--no-merges", "--format=%H%x1f%s%x1f%an%x1e", span,
              capture_output=True, text=True, encoding="utf-8").stdout
    return [tuple(entry.strip().split("\x1f")) for entry in out.split("\x1e") if entry.strip()]


def changelog(entries, repository, since, release_version, server_url="https://github.com"):
    """Markdown list of the commits with links to them."""
    base = f"{server_url.rstrip('/')}/{repository}"
    lines = ["## Изменения\n"]
    for sha, subject, author in entries:
        lines.append(f"- {subject.replace('<', '&lt;')} ([`{sha[:7]}`]({base}/commit/{sha})) — {author}")
    if not entries:
        lines.append("- Без изменений в коде.")
    if since:
        lines.append(f"\n[Все изменения {since}…v{release_version}]({base}/compare/{since}...v{release_version})")
    return "\n".join(lines) + "\n"


def notes():
    path = ROOT / "target/release-notes.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    repository = os.environ["GH_REPO"]
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
    release_version = version()
    since = previous_tag(release_version)
    path.write_text(download_table(repository, release_version, server)
                    + changelog(commits(since), repository, since, release_version, server),
                    encoding="utf-8", newline="\n")
    print(f"Generated release notes since {since or 'the first commit'}: {path}")


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
    commands = {"validate": validate, "build": build, "finalize": finalize, "notes": notes}
    if len(sys.argv) != 2 or sys.argv[1] not in commands:
        sys.exit("usage: release.py validate|build|finalize|notes")
    commands[sys.argv[1]]()

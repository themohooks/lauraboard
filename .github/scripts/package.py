"""Package already smoke-tested release binaries without recompiling Rust."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

import release

INPUT = release.ROOT / "target/build-input"
DEB_ARCH = {"x86_64": "amd64", "x86": "i386", "aarch64": "arm64"}
RPM_ARCH = {"x86_64": "x86_64", "x86": "i686", "aarch64": "aarch64"}
APPIMAGE_ARCH = {"x86_64": "x86_64", "x86": "i386", "aarch64": "aarch64"}
MULTIARCH = {"x86_64": "x86_64-linux-gnu", "x86": "i386-linux-gnu", "aarch64": "aarch64-linux-gnu"}
# Slint/winit loads these at runtime, so an ELF dependency scan alone misses them.
RUNTIME_LIBRARIES = ("libfontconfig.so.1", "libxkbcommon.so.0", "libxkbcommon-x11.so.0",
                     "libwayland-client.so.0", "libwayland-cursor.so.0", "libwayland-egl.so.1")
DESKTOP = """[Desktop Entry]
Type=Application
Name=LauraBoard
Comment=TV playout and broadcast automation
Exec=lauraboard
Icon=lauraboard
Terminal=false
Categories=AudioVideo;Video;
"""


def write(path, contents, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents, encoding="utf-8", newline="\n")
    path.chmod(mode)


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def unpack(archive, destination):
    with tarfile.open(archive) as package:
        package.extractall(destination, filter="data")
    stage, = destination.iterdir()
    # OS package managers own removal; do not ship the per-user uninstaller.
    (stage / "uninstall").unlink(missing_ok=True)
    return stage


def installed_tree(stage, root):
    shutil.copytree(stage, root / "opt/lauraboard")
    write(root / "usr/bin/lauraboard", '#!/bin/sh\nexec /opt/lauraboard/lauraboard "$@"\n', 0o755)
    write(root / "usr/share/applications/lauraboard.desktop", DESKTOP)
    icon = root / "usr/share/icons/hicolor/scalable/apps/lauraboard.svg"
    icon.parent.mkdir(parents=True)
    shutil.copy2(stage / "lauraboard.svg", icon)


def deb(stage, destination, version, arch, temporary):
    root = temporary / "deb"
    installed_tree(stage, root)
    # Let dpkg derive versioned ELF dependencies, then include dlopen libraries.
    write(temporary / "debian/control", "Source: lauraboard\n\nPackage: lauraboard\nArchitecture: any\nDescription: TV playout\n")
    binaries = [stage / "lauraboard", *(stage / tool for tool in ("ffmpeg", "ffprobe") if (stage / tool).exists())]
    dependencies = run("dpkg-shlibdeps", "-O", *(f"-e{binary}" for binary in binaries),
                       cwd=temporary, capture_output=True, text=True).stdout.strip().removeprefix("shlibs:Depends=")
    dependencies += ", libfontconfig1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, libwayland-egl1, libgl1"
    write(root / "DEBIAN/control", f"""Package: lauraboard
Version: {version.replace('-', '~', 1)}
Section: video
Priority: optional
Architecture: {DEB_ARCH[arch]}
Maintainer: LauraBoard contributors
Depends: {dependencies}
Description: TV playout and broadcast automation
""")
    run("dpkg-deb", "--root-owner-group", "--build", root, destination)
    run("dpkg-deb", "--info", destination)
    extracted = temporary / "deb-check"
    run("dpkg-deb", "--extract", destination, extracted)
    release.smoke(extracted / "opt/lauraboard/lauraboard", version)


def rpm(stage, destination, version, arch, temporary):
    root = temporary / "rpm-root"
    installed_tree(stage, root)
    top = temporary / "rpmbuild"
    top.mkdir()
    spec = temporary / "lauraboard.spec"
    library_suffix = "" if arch == "x86" else "()(64bit)"
    dependencies = ", ".join(library + library_suffix for library in (*RUNTIME_LIBRARIES, "libGL.so.1"))
    write(spec, f"""%global __os_install_post %{{nil}}
%global _build_id_links none
Name: lauraboard
Version: {version.replace('-', '~', 1)}
Release: 1
Summary: TV playout and broadcast automation
License: MIT
Requires: {dependencies}

%description
LauraBoard desktop application and CLI.

%prep
%build
%install
mkdir -p %{{buildroot}}
cp -a "{root}/." %{{buildroot}}/

%files
/opt/lauraboard
/usr/bin/lauraboard
/usr/share/applications/lauraboard.desktop
/usr/share/icons/hicolor/scalable/apps/lauraboard.svg
""")
    run("rpmbuild", "-bb", "--define", f"_topdir {top}", "--target", RPM_ARCH[arch], spec)
    package, = (top / "RPMS").rglob("*.rpm")
    shutil.copy2(package, destination)
    run("rpm", "-qp", "--info", destination)
    # Inspect the packaged payload and run the packaged binary without installing it.
    extracted = temporary / "rpm-check"
    extracted.mkdir()
    with tempfile.TemporaryFile() as payload:
        run("rpm2cpio", destination, stdout=payload)
        payload.seek(0)
        run("cpio", "-id", "--quiet", cwd=extracted, stdin=payload)
    release.smoke(extracted / "opt/lauraboard/lauraboard", version)


def linuxdeploy(arch, temporary):
    # Verify the bytes against GitHub's digest for the selected upstream asset.
    request = urllib.request.Request("https://api.github.com/repos/linuxdeploy/linuxdeploy/releases/tags/continuous",
                                     headers={"User-Agent": "LauraBoard-build", **(
                                         {"Authorization": f"Bearer {os.environ['GH_TOKEN']}"} if os.environ.get("GH_TOKEN") else {})})
    with urllib.request.urlopen(request, timeout=120) as response:
        metadata = json.load(response)
    name = f"linuxdeploy-{APPIMAGE_ARCH[arch]}.AppImage"
    asset, = (asset for asset in metadata["assets"] if asset["name"] == name)
    digest = asset.get("digest", "")
    if not digest.startswith("sha256:"):
        raise ValueError(f"Missing upstream SHA256 digest for {name}")
    path = temporary / name
    with urllib.request.urlopen(asset["browser_download_url"], timeout=120) as response, path.open("wb") as output:
        shutil.copyfileobj(response, output)
    with path.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != digest.removeprefix("sha256:"):
            raise ValueError(f"linuxdeploy checksum mismatch: {name}")
    path.chmod(0o755)
    return path


def appimage(stage, destination, version, arch, temporary):
    root = temporary / "LauraBoard.AppDir"
    shutil.copytree(stage, root / "usr/bin")
    desktop = temporary / "lauraboard.desktop"
    write(desktop, DESKTOP)
    run("desktop-file-validate", desktop)
    tool = linuxdeploy(arch, temporary)
    environment = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN="1", ARCH=APPIMAGE_ARCH[arch],
                       LDAI_OUTPUT=str(destination), LINUXDEPLOY_OUTPUT_VERSION=version)
    libraries = [argument for library in RUNTIME_LIBRARIES for argument in (
        "--library", str(Path("/usr/lib") / MULTIARCH[arch] / library))]
    run(tool, "--appdir", root, "--executable", root / "usr/bin/lauraboard",
        "--desktop-file", desktop, "--icon-file", stage / "lauraboard.svg", *libraries, "--output", "appimage",
        cwd=temporary, env=environment)
    # Check CLI inside the actual AppImage, without requiring FUSE on the runner.
    run(destination, "--appimage-extract", cwd=temporary)
    extracted = temporary / "squashfs-root"
    output = run(extracted / "AppRun", "--version", env=environment, capture_output=True, text=True).stdout.strip()
    if output != f"lauraboard {version}":
        raise ValueError(f"Unexpected AppImage CLI version: {output}")


def aur(stage, destination, version, arch, temporary):
    root = temporary / "aur"
    root.mkdir()
    # A self-contained source package works for manual Actions builds as well as releases.
    payload = root / "lauraboard-payload.tar.gz"
    release.archive(stage, payload)
    with payload.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    aur_arch = RPM_ARCH[arch]
    pkgver = version.replace("-", "_")
    dependencies = ["glibc", "gcc-libs", "fontconfig", "libxkbcommon", "libxkbcommon-x11", "wayland", "libglvnd", "openssl", "dbus"]
    write(root / "lauraboard.desktop", DESKTOP)
    desktop_digest = hashlib.sha256(DESKTOP.encode()).hexdigest()
    write(root / "PKGBUILD", f"""pkgname=lauraboard-bin
pkgver={pkgver}
pkgrel=1
pkgdesc='TV playout and broadcast automation'
arch=('{aur_arch}')
license=('MIT')
depends=({' '.join(repr(name) for name in dependencies)})
provides=('lauraboard')
conflicts=('lauraboard')
options=('!strip')
source=('lauraboard-payload.tar.gz' 'lauraboard.desktop')
sha256sums=('{digest}' '{desktop_digest}')

package() {{
    install -dm755 "$pkgdir/opt/lauraboard" "$pkgdir/usr/bin"
    cp -a "$srcdir/lauraboard-payload/." "$pkgdir/opt/lauraboard/"
    printf '#!/bin/sh\\nexec /opt/lauraboard/lauraboard "$@"\\n' > "$pkgdir/usr/bin/lauraboard"
    chmod 755 "$pkgdir/usr/bin/lauraboard"
    install -Dm644 "$srcdir/lauraboard.desktop" "$pkgdir/usr/share/applications/lauraboard.desktop"
    install -Dm644 "$pkgdir/opt/lauraboard/lauraboard.svg" "$pkgdir/usr/share/icons/hicolor/scalable/apps/lauraboard.svg"
}}
""")
    write(root / ".SRCINFO", f"""pkgbase = lauraboard-bin
\tpkgdesc = TV playout and broadcast automation
\tpkgver = {pkgver}
\tpkgrel = 1
\tarch = {aur_arch}
\tlicense = MIT
""" + "".join(f"\tdepends = {name}\n" for name in dependencies) + f"""\tprovides = lauraboard
\tconflicts = lauraboard
\toptions = !strip
\tsource = lauraboard-payload.tar.gz
\tsource = lauraboard.desktop
\tsha256sums = {digest}
\tsha256sums = {desktop_digest}

pkgname = lauraboard-bin
""")
    release.archive(root, destination)


def package():
    target = os.environ["RELEASE_TARGET"]
    fmt = os.environ["PACKAGE_FORMAT"]
    system, arch = release.TARGETS[target]
    version = release.version()
    prefix = f"lauraboard-{version}"
    release.ASSETS.mkdir(parents=True, exist_ok=True)
    if any(release.ASSETS.iterdir()):
        raise ValueError("Package output must be empty")
    extension = ".zip" if system == "windows" else ".tar.gz"
    if fmt in ("setup", "portable", "server"):
        name = (f"{prefix}-{system}-{arch}-setup" + (".exe" if system == "windows" else "")) if fmt == "setup" else (
            f"{prefix}-{target}-{'desktop' if fmt == 'portable' else 'server'}{extension}")
        shutil.copy2(INPUT / name, release.ASSETS / name)
        return
    if system != "linux":
        raise ValueError(f"{fmt} requires Linux")
    extensions = {"appimage": "AppImage", "debpackage": "deb", "rpmpackage": "rpm", "aur": "aur.tar.gz"}
    destination = release.ASSETS / f"{prefix}-{target}.{extensions[fmt]}"
    with tempfile.TemporaryDirectory(prefix="lauraboard-package-") as temporary:
        temporary = Path(temporary)
        stage = unpack(INPUT / f"{prefix}-{target}-desktop.tar.gz", temporary / "unpacked")
        {"appimage": appimage, "debpackage": deb, "rpmpackage": rpm, "aur": aur}[fmt](
            stage, destination, version, arch, temporary)
    if not destination.is_file() or not destination.stat().st_size:
        raise ValueError(f"Missing package: {destination}")


if __name__ == "__main__":
    package()

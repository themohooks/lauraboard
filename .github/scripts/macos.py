"""Native macOS .app/DMG packaging, including relocatable FFmpeg libraries."""
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def copy_tools(destination, frameworks=None, notices=None):
    """Copy Homebrew tools plus non-system Mach-O dependencies; rewrite load paths."""
    frameworks = frameworks or destination.parent / "Frameworks"
    frameworks.mkdir(parents=True, exist_ok=True)
    copied = {}
    origins = {}

    def install(source, target):
        source = source.resolve()
        if source in copied:
            return copied[source]
        if target.name in origins and origins[target.name] != source:
            raise ValueError(f"Conflicting bundled library: {target.name}")
        origins[target.name] = source
        copied[source] = target
        shutil.copy2(source, target)
        target.chmod(0o755)
        # Brew signatures cease to be valid after changing dylib paths.
        subprocess.run(["codesign", "--remove-signature", str(target)], check=False, capture_output=True)
        load = run("otool", "-l", source, capture_output=True, text=True).stdout
        rpaths = []
        lines = load.splitlines()
        for i, line in enumerate(lines):
            if "cmd LC_RPATH" in line:
                rpaths.append(lines[i + 2].strip().removeprefix("path ").split(" (offset", 1)[0])
        dependencies = run("otool", "-L", source, capture_output=True, text=True).stdout.splitlines()[1:]
        for line in dependencies:
            name = line.strip().split(" (", 1)[0]
            if name.startswith(("/System/", "/usr/lib/")):
                continue
            candidates = [Path(name.replace("@loader_path", str(source.parent)))]
            if name.startswith("@rpath/"):
                tail = name.removeprefix("@rpath/")
                candidates = [Path(p.replace("@loader_path", str(source.parent)).replace("@executable_path", str(source.parent))) / tail for p in rpaths]
                candidates += [source.parent / tail, Path("/opt/homebrew/lib") / tail, Path("/usr/local/lib") / tail]
            dependency = next((p.resolve() for p in candidates if p.is_file()), None)
            if dependency is None:
                raise ValueError(f"Cannot resolve {name} needed by {source}")
            if dependency == source:
                run("install_name_tool", "-id", f"@rpath/{target.name}", target)
                continue
            bundled = install(dependency, frameworks / dependency.name)
            relative = os.path.relpath(bundled, target.parent)
            run("install_name_tool", "-change", name, f"@loader_path/{relative}", target)
        run("codesign", "--force", "--sign", os.environ.get("MACOS_SIGNING_IDENTITY", "-"), target)
        return target

    for tool in ("ffmpeg", "ffprobe"):
        found = shutil.which(tool)
        if not found:
            raise ValueError(f"Install native {tool} before packaging macOS")
        install(Path(found), destination / tool)
    prefix = Path(run("brew", "--prefix", "ffmpeg", capture_output=True, text=True).stdout.strip())
    notices = notices or destination.parent / "Resources/licenses/ffmpeg"
    notices.mkdir(parents=True, exist_ok=True)
    docs = prefix / "share/doc/ffmpeg"
    if docs.is_dir():
        shutil.copytree(docs, notices, dirs_exist_ok=True)
    for name in ("LICENSE", "COPYING.GPLv2", "COPYING.GPLv3"):
        if (prefix / name).is_file():
            shutil.copy2(prefix / name, notices / name)
    (notices / "BUILD.txt").write_text(run("ffmpeg", "-buildconf", capture_output=True, text=True).stderr, encoding="utf-8")


def info(version):
    return {"CFBundleName": "LauraBoard", "CFBundleDisplayName": "LauraBoard", "CFBundleIdentifier": "org.lauraboard.desktop",
            "CFBundleExecutable": "lauraboard", "CFBundlePackageType": "APPL", "CFBundleShortVersionString": version.split("-")[0],
            "CFBundleVersion": version.split("-")[0], "LSMinimumSystemVersion": "15.0", "CFBundleIconFile": "lauraboard.icns", "NSHighResolutionCapable": True,
            "NSLocalNetworkUsageDescription": "Вещание выбранного канала и управление LauraBoard в локальной сети."}


def desktop(root, assets, version, target):
    run("cargo", "build", "--release", "--locked", "-p", "lauraboard", "-p", "lb-webview")
    stage = root / "target/macos-desktop"
    if stage.exists():
        shutil.rmtree(stage)
    app = stage / "LauraBoard.app"
    binary_dir = app / "Contents/MacOS"
    binary_dir.mkdir(parents=True)
    shutil.copy2(root / "target/release/lauraboard", binary_dir / "lauraboard")
    shutil.copy2(root / "target/release/lauraboard-webview", binary_dir / "lauraboard-webview")
    with (app / "Contents/Info.plist").open("wb") as output:
        plistlib.dump(info(version), output)
    (app / "Contents/PkgInfo").write_bytes(b"APPL????")
    resources = app / "Contents/Resources"
    resources.mkdir()
    ico = (root / "crates/lb-gui/assets/app/lauraboard.ico").read_bytes()
    count = struct.unpack_from("<H", ico, 4)[0]
    for i in range(count):
        entry = 6 + i * 16
        if ico[entry] == 0:
            size, offset = struct.unpack_from("<II", ico, entry + 8)
            png = ico[offset:offset+size]
            chunk = b"ic08" + struct.pack(">I", size + 8) + png
            (resources / "lauraboard.icns").write_bytes(b"icns" + struct.pack(">I", len(chunk)+8) + chunk)
            break
    import fetch_web_runtime
    runtime = root / "target/debug/runtime"
    fetch_web_runtime.prepare(runtime, "macos", "aarch64" if target.startswith("aarch64") else "x86_64")
    shutil.copytree(runtime / "ruffle", resources / "ruffle")
    licenses = resources / "licenses"
    licenses.mkdir(exist_ok=True)
    gui_assets = root / "crates/lb-gui/assets"
    for source, name in ((gui_assets / "fonts/OFL-Roboto.txt", "OFL-Roboto.txt"), (gui_assets / "fonts/OFL-RobotoMono.txt", "OFL-RobotoMono.txt"), (gui_assets / "icons/LICENSE", "MaterialSymbols-Apache-2.0.txt")):
        shutil.copy2(source, licenses / name)
    copy_tools(binary_dir)
    # Sign the helper before its outer app bundle.
    identity = os.environ.get("MACOS_SIGNING_IDENTITY", "-")
    run("codesign", "--force", "--sign", identity, binary_dir / "lauraboard-webview")
    run("codesign", "--force", "--sign", identity, app)
    run("codesign", "--verify", "--deep", "--strict", app)
    return stage


def dmg(stage, output):
    image = stage.parent / "macos-dmg"
    if image.exists():
        shutil.rmtree(image)
    image.mkdir()
    shutil.copytree(stage / "LauraBoard.app", image / "LauraBoard.app", symlinks=True)
    (image / "Applications").symlink_to("/Applications")
    run("hdiutil", "create", "-volname", "LauraBoard", "-srcfolder", image, "-format", "UDZO", "-ov", output)

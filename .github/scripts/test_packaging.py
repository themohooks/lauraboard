import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import subprocess
import tarfile
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).parent))
import build_matrix
import release

# Avoid shadowing Python's packaging module in the test runner.
spec = importlib.util.spec_from_file_location("release_package", Path(__file__).with_name("package.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class MatrixTests(unittest.TestCase):
    def test_all_architectures_and_formats_are_independent(self):
        matrix = build_matrix.matrices()
        builds = matrix["build"]["include"]
        jobs = matrix["package"]["include"]
        self.assertEqual(len(builds), 5)
        self.assertEqual(len(jobs), 27)
        for build in builds:
            selected = [job["format"] for job in jobs if job["target"] == build["target"]]
            self.assertCountEqual(selected, build_matrix.FORMATS[build["platform"]])
            self.assertEqual(json.loads(build["formats"]), selected)
        self.assertEqual(len({(job["target"], job["format"]) for job in jobs}), len(jobs))
        for section in ("build", "package"):
            self.assertFalse(any(job["target"] == "i686-pc-windows-msvc" for job in matrix[section]["include"]))

    def test_single_selection_and_native_i686_hosts(self):
        matrix = build_matrix.matrices("linux", "x86", "debpackage")
        build, = matrix["build"]["include"]
        self.assertEqual(build["toolchain"], "1.96.0-i686-unknown-linux-gnu")
        job, = matrix["package"]["include"]
        self.assertEqual(job["format"], "debpackage")
        self.assertEqual(job["target"], "i686-unknown-linux-gnu")

    def test_incompatible_and_unknown_selections_are_rejected(self):
        for args in [("windows", "x86", "all"), ("windows", "all", "appimage"), ("linux", "arm", "all"), ("bad", "all", "all")]:
            with self.assertRaises(ValueError):
                build_matrix.matrices(*args)


class PackageTests(unittest.TestCase):
    def test_selected_formats_reuse_local_build_and_preserve_separate_artifacts(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            assets = root / "target/release-assets"
            assets.mkdir(parents=True)
            names = {
                "setup": "lauraboard-1.2.3-windows-aarch64-setup.exe",
                "portable": "lauraboard-1.2.3-aarch64-pc-windows-msvc-desktop.zip",
                "server": "lauraboard-1.2.3-aarch64-pc-windows-msvc-server.zip",
            }
            for fmt, name in names.items():
                (assets / name).write_bytes(fmt.encode())
            with mock.patch.object(release, "ROOT", root), mock.patch.object(release, "ASSETS", assets), \
                    mock.patch.object(release, "version", return_value="1.2.3"), \
                    mock.patch.dict("os.environ", {"RELEASE_TARGET": "aarch64-pc-windows-msvc", "PACKAGE_FORMAT": "unchanged"}):
                package.package_selected(list(names))
                self.assertEqual(package.os.environ["PACKAGE_FORMAT"], "unchanged")
            self.assertCountEqual([p.name for p in assets.iterdir()], names.values())
            for fmt, name in names.items():
                directory = root / "target/packages" / fmt
                self.assertEqual([p.name for p in directory.iterdir()], [name])
                self.assertEqual((directory / name).read_bytes(), fmt.encode())

    def test_invalid_format_selection_does_not_start_packaging(self):
        for formats in ([], ["setup", "setup"], ["setup", "bad"], "setup", [{}]):
            with self.subTest(formats=formats), mock.patch.object(package, "package") as build:
                with self.assertRaises(ValueError):
                    package.package_selected(formats)
                build.assert_not_called()

    def test_dependency_scan_failure_keeps_captured_diagnostics(self):
        failure = subprocess.CalledProcessError(2, ["dpkg-shlibdeps"], stderr="cannot find libwebkit2gtk-4.1.so.0")
        diagnostics = io.StringIO()
        with mock.patch.object(package.subprocess, "run", side_effect=failure), mock.patch.object(sys, "stderr", diagnostics):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                package.run("dpkg-shlibdeps", "-O", capture_output=True, text=True)
        self.assertIs(raised.exception, failure)
        self.assertIn("cannot find libwebkit2gtk-4.1.so.0", diagnostics.getvalue())

    def stage(self, root):
        stage = root / "stage"
        stage.mkdir()
        for name in ("lauraboard", "ffmpeg", "ffprobe", "uninstall"):
            (stage / name).write_bytes(b"binary")
        (stage / "lauraboard.svg").write_text("<svg/>")
        return stage

    def test_os_packages_remove_per_user_uninstaller_and_keep_modes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            stage = self.stage(root)
            portable = root / "portable.tar.gz"
            release.archive(stage, portable)
            extracted = package.unpack(portable, root / "unpacked")
            self.assertFalse((extracted / "uninstall").exists())
            package.installed_tree(extracted, root / "installed")
            self.assertEqual((root / "installed/opt/lauraboard/ffprobe").read_bytes(), b"binary")
            launcher = (root / "installed/usr/bin/lauraboard").read_text()
            self.assertIn('exec /opt/lauraboard/lauraboard "$@"', launcher)
            self.assertIn("Exec=lauraboard", (root / "installed/usr/share/applications/lauraboard.desktop").read_text())

    def test_aur_sources_are_self_contained_and_checksummed_for_each_arch(self):
        for arch, expected in [("x86_64", "x86_64"), ("x86", "i686"), ("aarch64", "aarch64")]:
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                stage = self.stage(root)
                (stage / "uninstall").unlink()
                destination = root / "release.aur.tar.gz"
                package.aur(stage, destination, "1.2.3-rc.1", arch, root)
                with tarfile.open(destination) as archive:
                    names = {Path(name).name for name in archive.getnames()}
                    self.assertTrue({"PKGBUILD", ".SRCINFO", "lauraboard-payload.tar.gz", "lauraboard.desktop"} <= names)
                aur = root / "aur"
                digest = hashlib.sha256((aur / "lauraboard-payload.tar.gz").read_bytes()).hexdigest()
                for name in ("PKGBUILD", ".SRCINFO"):
                    text = (aur / name).read_text()
                    self.assertIn(digest, text)
                    self.assertIn(expected, text)
                    self.assertIn("1.2.3_rc.1", text)
                with tarfile.open(aur / "lauraboard-payload.tar.gz") as payload:
                    self.assertEqual(payload.getmember("lauraboard-payload/lauraboard").mode, 0o755)
                    self.assertFalse(any(name.endswith("/uninstall") for name in payload.getnames()))

    def test_setup_and_portable_are_emitted_as_separate_single_file_outputs(self):
        for target in release.TARGETS:
            # Linux ships AppImage / deb / rpm packages instead (see above).
            if "darwin" in target or "linux" in target:
                continue
            system, arch = release.TARGETS[target]
            for fmt in ("setup", "portable", "server"):
                with self.subTest(target=target, fmt=fmt), tempfile.TemporaryDirectory() as temp:
                    root = Path(temp)
                    inputs, assets = root / "input", root / "assets"
                    inputs.mkdir()
                    extension = ".zip" if system == "windows" else ".tar.gz"
                    name = (f"lauraboard-1.2.3-{system}-{arch}-setup" + (".exe" if system == "windows" else "")) if fmt == "setup" else (
                        f"lauraboard-1.2.3-{target}-{'desktop' if fmt == 'portable' else 'server'}{extension}")
                    (inputs / name).write_bytes(b"asset")
                    with mock.patch.object(package, "INPUT", inputs), mock.patch.object(release, "ASSETS", assets), \
                            mock.patch.object(release, "version", return_value="1.2.3"), \
                            mock.patch.dict("os.environ", {"RELEASE_TARGET": target, "PACKAGE_FORMAT": fmt}):
                        package.package()
                    self.assertEqual([path.name for path in assets.iterdir()], [name])
                    self.assertIn(name, release.expected_assets("1.2.3"))


if __name__ == "__main__":
    unittest.main()

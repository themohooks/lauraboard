"""Opt-in real Linux packager tests, using a tiny ELF fixture instead of the GUI."""

import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
spec = importlib.util.spec_from_file_location("release_package", Path(__file__).with_name("package.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


@unittest.skipUnless(os.environ.get("LB_PACKAGE_INTEGRATION") == "1", "opt-in native Linux packager tests")
class NativePackageTests(unittest.TestCase):
    def test_native_packages_and_aur_payload(self):
        arch = os.environ.get("PACKAGE_TEST_ARCH", "x86_64")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            stage = root / "stage"
            stage.mkdir()
            source = root / "fixture.c"
            source.write_text('#include <stdio.h>\nint main(void) { puts("lauraboard 1.2.3-rc.1"); return 0; }\n')
            flags = ["-m32"] if arch == "x86" else []
            subprocess.run(["gcc", *flags, str(source), "-o", str(stage / "lauraboard")], check=True)
            shutil.copy2(package.release.ROOT / "crates/lb-gui/assets/app/lauraboard.svg", stage / "lauraboard.svg")
            for fmt, suffix in [(package.deb, "deb"), (package.rpm, "rpm"),
                                (package.appimage, "AppImage"), (package.aur, "aur.tar.gz")]:
                with self.subTest(format=suffix):
                    temporary = root / suffix
                    temporary.mkdir()
                    destination = root / f"fixture.{suffix}"
                    fmt(stage, destination, "1.2.3-rc.1", arch, temporary)
                    self.assertGreater(destination.stat().st_size, 0)
                    if suffix == "deb":
                        declared = subprocess.check_output(["dpkg-deb", "-f", str(destination), "Architecture"], text=True).strip()
                        self.assertEqual(declared, package.DEB_ARCH[arch])
                    elif suffix == "rpm":
                        declared = subprocess.check_output(["rpm", "-qp", "--queryformat", "%{ARCH}", str(destination)], text=True)
                        self.assertEqual(declared, package.RPM_ARCH[arch])
                    if suffix == "aur.tar.gz":
                        aur = temporary / "aur"
                        subprocess.run(["bash", "-n", str(aur / "PKGBUILD")], check=True)
                        src = temporary / "src"
                        src.mkdir()
                        with tarfile.open(aur / "lauraboard-payload.tar.gz") as payload:
                            payload.extractall(src, filter="data")
                        shutil.copy2(aur / "lauraboard.desktop", src / "lauraboard.desktop")
                        environment = dict(os.environ, srcdir=str(src), pkgdir=str(temporary / "pkg"))
                        subprocess.run(["bash", "-c", 'source "$1"; package', "--", str(aur / "PKGBUILD")],
                                       env=environment, check=True)
                        package.release.smoke(temporary / "pkg/opt/lauraboard/lauraboard", "1.2.3-rc.1")


if __name__ == "__main__":
    unittest.main()

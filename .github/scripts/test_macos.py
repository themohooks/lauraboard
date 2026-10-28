import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import macos


class MacosToolsTests(unittest.TestCase):
    def test_signed_libraries_are_edited_together_then_resigned(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "brew"
            source.mkdir()
            library = source / "libfixture.dylib"
            library.write_bytes(b"signed library")
            for tool in ("ffmpeg", "ffprobe"):
                (source / tool).write_bytes(b"signed executable")
            destination = root / "bundle/bin"
            destination.mkdir(parents=True)
            commands = []

            def run(*args, **kwargs):
                args = tuple(map(str, args))
                commands.append(args)
                output = ""
                if args[:2] == ("otool", "-L"):
                    output = f"{args[2]}:\n\t{library} (compatibility version 1.0.0)\n"
                if args[:2] == ("brew", "--prefix"):
                    output = str(source)
                return subprocess.CompletedProcess(args, 0, stdout=output, stderr="build config")

            with mock.patch.object(macos, "run", side_effect=run), \
                    mock.patch.object(macos.shutil, "which", side_effect=lambda tool: str(source / tool)):
                macos.copy_tools(destination)
            edits = [command for command in commands if command[0] == "install_name_tool"]
            self.assertEqual(len(edits), 3)
            self.assertTrue(any(command[1:3] == ("-id", "@rpath/libfixture.dylib") for command in edits))
            self.assertTrue(any("@loader_path/../Frameworks/libfixture.dylib" in command for command in edits))
            self.assertFalse(any("--remove-signature" in command for command in commands))
            for edit in edits:
                signing = next(c for c in commands if c[:2] == ("codesign", "--force") and c[-1] == edit[-1])
                verification = next(c for c in commands if c[:2] == ("codesign", "--verify") and c[-1] == edit[-1])
                self.assertLess(commands.index(edit), commands.index(signing))
                self.assertLess(commands.index(signing), commands.index(verification))

    @unittest.skipUnless(os.sys.platform == "darwin", "requires native Apple tools")
    def test_native_signed_dylib_relocation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "brew"
            source.mkdir()
            library = source / "libfixture.dylib"
            (source / "fixture.c").write_text("int fixture(void) { return 0; }")
            (source / "main.c").write_text("int fixture(void); int main(void) { return fixture(); }")
            macos.run("clang", "-dynamiclib", source / "fixture.c", "-Wl,-headerpad_max_install_names",
                      "-Wl,-install_name," + str(library), "-o", library)
            macos.run("codesign", "--force", "--sign", "-", library)
            for tool in ("ffmpeg", "ffprobe"):
                macos.run("clang", source / "main.c", library, "-Wl,-headerpad_max_install_names", "-o", source / tool)
                macos.run("codesign", "--force", "--sign", "-", source / tool)
            destination = root / "bundle/bin"
            destination.mkdir(parents=True)
            native_run = macos.run

            def run(*args, **kwargs):
                if args[0] == "brew":
                    return subprocess.CompletedProcess(args, 0, stdout=str(source))
                if args[0] == "ffmpeg":
                    return subprocess.CompletedProcess(args, 0, stderr="fixture")
                return native_run(*args, **kwargs)

            with mock.patch.object(macos, "run", side_effect=run), \
                    mock.patch.object(macos.shutil, "which", side_effect=lambda tool: str(source / tool)), \
                    mock.patch.dict(os.environ, {"MACOS_SIGNING_IDENTITY": "-"}):
                macos.copy_tools(destination)
            # Prove the relocated tools no longer depend on the source library.
            library.unlink()
            for tool in ("ffmpeg", "ffprobe"):
                native_run(destination / tool)


if __name__ == "__main__":
    unittest.main()

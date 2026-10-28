import struct
import tempfile
import unittest
import zlib
from pathlib import Path
from unittest.mock import patch
import generate_test_cards as cards
import macos


def pixels(path):
    data = path.read_bytes()
    width, height = struct.unpack_from(">II", data, 16)
    compressed = b""
    at = 8
    while at < len(data):
        size = struct.unpack_from(">I", data, at)[0]
        if data[at+4:at+8] == b"IDAT":
            compressed += data[at+8:at+8+size]
        at += size + 12
    return width, height, zlib.decompress(compressed)


class TestPatterns(unittest.TestCase):
    def test_exact_rgb_patches_and_geometry(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(cards, "OUT", Path(directory)):
                cards.generate()
            w, h, data = pixels(Path(directory) / "test-color-bars.png")
            self.assertEqual((w, h), (1920, 1080))
            def pixel(x, y):
                at = y * (w * 3 + 1) + 1 + x * 3
                return tuple(data[at:at+3])
            self.assertEqual(pixel(10, 10), (191, 191, 191))
            self.assertEqual(pixel(400, 10), (191, 191, 0))
            self.assertEqual(pixel(1910, 10), (0, 0, 191))
            self.assertEqual(pixel(10, 850), (0, 0, 0))
            self.assertEqual(pixel(1910, 850), (255, 255, 255))
            self.assertEqual(pixels(Path(directory) / "test-geometry.png")[:2], (1920, 1080))

    def test_native_macos_bundle_metadata(self):
        info = macos.info("1.2.3-rc.1")
        self.assertEqual(info["CFBundleExecutable"], "lauraboard")
        self.assertEqual(info["CFBundlePackageType"], "APPL")
        self.assertEqual(info["CFBundleShortVersionString"], "1.2.3")
        self.assertTrue(info["NSHighResolutionCapable"])


if __name__ == "__main__":
    unittest.main()

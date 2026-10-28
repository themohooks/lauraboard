import unittest

from ci_resources import GIB, settings


class ResourceTests(unittest.TestCase):
    def test_standard_runner_sizes(self):
        for cpus, gib, expected in [(4, 16, "2"), (4, 14, "2"), (3, 7, "1"), (2, 8, "1")]:
            with self.subTest(cpus=cpus, gib=gib):
                self.assertEqual(settings(cpus, gib * GIB)["CARGO_BUILD_JOBS"], expected)

    def test_lightweight_jobs_and_x86(self):
        self.assertEqual(settings(4, 16 * GIB, desktop=False)["CARGO_BUILD_JOBS"], "4")
        self.assertEqual(settings(2, 8 * GIB, desktop=False)["CARGO_BUILD_JOBS"], "2")
        self.assertEqual(settings(4, 16 * GIB, x86=True)["CARGO_BUILD_JOBS"], "1")
        self.assertEqual(settings(1, GIB)["CARGO_BUILD_JOBS"], "1")

    def test_release_changes_are_opt_in(self):
        self.assertNotIn("CARGO_PROFILE_RELEASE_LTO", settings(4, 16 * GIB))
        values = settings(4, 16 * GIB, fast_release=True)
        self.assertEqual(values["CARGO_PROFILE_RELEASE_LTO"], "off")
        self.assertEqual(values["CARGO_PROFILE_RELEASE_CODEGEN_UNITS"], "16")
        self.assertNotIn("CARGO_PROFILE_RELEASE_OPT_LEVEL", values)


if __name__ == "__main__":
    unittest.main()

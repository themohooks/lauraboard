"""Opt-in native test of yt-dlp's EJS adapter, without external JS engines.

Python is a CI test dependency only. The installed application uses the
official self-contained yt-dlp executable to load the embedded plugin.
"""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.environ.get("LB_YTDLP_WEBVIEW_TEST") == "1", "opt-in native yt-dlp/WebView test")
class WebViewProviderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        sys.path.insert(0, str(ROOT / "target/ytdlp-provider-test"))
        from yt_dlp.extractor.youtube.jsc.provider import JsChallengeProviderError
        import yt_dlp_ejs.yt.solver
        cls.solver = yt_dlp_ejs.yt.solver
        cls.error = JsChallengeProviderError
        path = ROOT / "crates/lb-engine/src/download/webview_jsc.py"
        spec = importlib.util.spec_from_file_location("yt_dlp_plugins.extractor.lauraboard_webview", path)
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
        cls.provider_class = module.LauraBoardWebViewJCP
        cls.helper = Path(os.environ.get("LB_TEST_WEBVIEW", ROOT / "target/debug" / (
            "lauraboard-webview.exe" if os.name == "nt" else "lauraboard-webview"))).resolve()
        if not cls.helper.is_file():
            raise AssertionError(f"Build the native WebView host first: {cls.helper}")

    def test_actual_ejs_scripts_execute_without_python_or_js_on_path(self):
        source = (self.solver.lib() + "\nObject.assign(globalThis, lib);\n" + self.solver.core()
                  + '\nconsole.log(JSON.stringify({solver:typeof jsc,answer:6*7,text:"Проверка"}));')
        provider = object.__new__(self.provider_class)
        with tempfile.TemporaryDirectory() as empty, mock.patch.dict(os.environ, {
                "LAURABOARD_WEBVIEW": str(self.helper), "PATH": empty}):
            value = json.loads(provider._run_js_runtime(source))
        self.assertEqual(value, {"solver": "function", "answer": 42, "text": "Проверка"})

    def test_js_exceptions_are_provider_errors(self):
        provider = object.__new__(self.provider_class)
        with mock.patch.dict(os.environ, {"LAURABOARD_WEBVIEW": str(self.helper)}):
            with self.assertRaisesRegex(self.error, "fixture exception"):
                provider._run_js_runtime("throw new Error('fixture exception');")


if __name__ == "__main__":
    unittest.main()

"""LauraBoard's yt-dlp plugin. EJS scripts are supplied/verified by yt-dlp.

The EJS base class is internal to yt-dlp: compatibility is covered by the
integration test when updating the downloader. No standalone Python needed
when loaded by the official yt-dlp executable.
"""
import json
import os
import subprocess

from yt_dlp.extractor.youtube.jsc._builtin.ejs import EJSBaseJCP
from yt_dlp.extractor.youtube.jsc.provider import (
    JsChallengeProviderError,
    register_preference,
    register_provider,
)


@register_provider
class LauraBoardWebViewJCP(EJSBaseJCP):
    PROVIDER_NAME = "lauraboard-webview"
    PROVIDER_VERSION = "1.0.0"
    JS_RUNTIME_NAME = "lauraboard-webview"
    BUG_REPORT_LOCATION = "LauraBoard"

    def is_available(self):
        return self._available and os.path.isfile(os.environ.get("LAURABOARD_WEBVIEW", ""))

    def _run_js_runtime(self, source, /):
        helper = os.environ.get("LAURABOARD_WEBVIEW")
        if not helper:
            raise JsChallengeProviderError("LauraBoard WebView component is unavailable", expected=True)
        try:
            result = subprocess.run(
                [helper, "javascript"], input=source, text=True, encoding="utf-8",
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=75,
                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
            )
            response = json.loads(result.stdout)
            if result.returncode or response.get("error"):
                raise JsChallengeProviderError(response.get("error") or "WebView execution failed", expected=True)
            value = response.get("result")
            if not isinstance(value, str):
                raise JsChallengeProviderError("WebView returned an invalid result", expected=True)
            return value
        except (OSError, subprocess.TimeoutExpired, ValueError) as error:
            raise JsChallengeProviderError(f"LauraBoard WebView: {error}", expected=True) from error


@register_preference(LauraBoardWebViewJCP)
def preference(provider, requests):
    return 10000

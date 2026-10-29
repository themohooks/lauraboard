# LauraBoard experimental Android client

Remote Slint operator GUI, without the playout engine, FFmpeg, yt-dlp or
desktop WebView. Branch: `experimental/android-app`.

## Build a debug APK on Windows

Prerequisites: Rust 1.92+, the `aarch64-linux-android` Rust target, a JDK with
`javac`, Android SDK platform 35, Build Tools 35.0.0, and stable NDK r27/r28. Android Studio's SDK
Manager can install the SDK/NDK. Keep Java and Cargo on `PATH`.

From the repository root:

```powershell
./crates/lb-android/build-debug.ps1
```

The script discovers the SDK from `ANDROID_HOME` or `%LOCALAPPDATA%/Android/Sdk`
and the NDK from `ANDROID_NDK_ROOT` or installed r27/r28 versions. Override with
`-Sdk <directory> -Ndk <directory>`. It sets build environment variables only
for the current process and restores them afterwards.

On first use it installs the Git revision `e67b501c` of rust-mobile/xbuild
under `target/android-tools`. xbuild also downloads its own SDK/sysroot cache;
the standard NDK supplies clang and Skia's Android toolchain. Internet access
is required on the first build. The native library is built by xbuild; the
installed SDK tools package and debug-sign the APK. Slint's Android backend uses Skia, so the first
build is substantially slower than an incremental build.

Output: `target/x/debug/android/lb-android.apk` (debug-signed, arm64-v8a).
Build artifacts and downloaded tools under `target` are not committed.

## Connect and test

Install with `adb install -r target/x/debug/android/lb-android.apk`, then open
**LauraBoard Experimental**. Android 8.0 / API 26 or later is required.
The application uses landscape orientation.

Enter the remote server's base URL, e.g. `http://192.168.1.10:8787`, and an
optional bearer token. `/api/v1` is added by the client. The server must be
reachable from the device. `127.0.0.1` refers to the Android device itself.
The Connect button checks `/api/v1/channels` on a background thread and shows
connection/authentication errors without opening the operator interface.

The URL and GUI preferences are stored in Android's private internal data
directory. The token is held only in memory. Restart the application to return
to the connection form and select another server.

File selection/save dialogs, desktop WebView login, external program launch,
tray integration and automatic installer updates are unavailable. File dialog
actions explain this limitation. Server-side operations remain remote: paths
entered in server forms refer to the server's filesystem.

This is a desktop GUI experiment: its existing horizontally scrollable screens
are reused, not redesigned for phones. Background execution is subject to the
Android process lifecycle; the remote broadcast engine continues independently.

The pinned xbuild cannot package `compileSdkVersion` against its API 27
resource table (`failed to locate key id compileSdkVersion`). The build script
recognizes that packaging error after native compilation and uses `aapt2`,
`zipalign` and `apksigner` from the installed SDK. Other xbuild errors remain
fatal. `AndroidManifest.xml` is the final APK manifest: minimum API 26, target
API 35, `INTERNET`, and explicit `usesCleartextTraffic=true` for LAN HTTP.
HTTPS is also supported. `manifest.yaml` configures only the xbuild stage.

Full debug symbols remain in the native Cargo output. The packaged library
has its debug sections stripped, as in normal Android debug packaging. The
debug keystore is generated locally under `target/android-package` and must
be retained to install subsequent builds over the existing app. This APK is
not a Play Store release.

Host regression checks:

```powershell
cargo check -p lb-gui
cargo test -p lb-gui --lib
```

For runtime verification, install on a device/emulator, check the form with an
unreachable URL and an invalid token, connect to a working server, navigate
screens, reopen the app, and verify that only the URL was remembered.

## Validation on 2026-10-29

- ARM64 debug APK built successfully through `build-debug.ps1`, including the
  SDK packaging workaround; APK size: 53,367,206 bytes.
- APK signature v2/v3 verified; manifest confirms debuggable, API 26 minimum,
  target SDK 35, Internet permission, cleartext HTTP, and NativeActivity.
- ELF64 AArch64 library exports `android_main` and `ANativeActivity_onCreate`;
  all strong undefined symbols exist in API 26's system libraries.
- Slint fonts/images are embedded in the native library; the APK contains the
  manifest, resource table, and `lib/arm64-v8a/liblb_android.so`.
- Desktop `cargo check -p lb-gui` passed. `cargo test -p lb-gui --lib`:
  49 passed, 0 failed, 1 ignored, including connection/authentication tests.
- No connected Android device or configured AVD was available; launch,
  touch interaction and connection to a real server remain unverified.

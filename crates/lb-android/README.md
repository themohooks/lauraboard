# LauraBoard experimental Android client

Remote Slint operator GUI, without the playout engine, FFmpeg, yt-dlp or
desktop WebView. Branch: `experimental/android-app`.

## Build a debug APK on Windows

Prerequisites: Rust 1.92+, the `aarch64-linux-android` Rust target, a JDK with
`javac`, Android SDK platform 35, and stable NDK r27/r28. Android Studio's SDK
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
is required on the first build. Slint's Android backend uses Skia, so the first
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

The pinned xbuild manifest model cannot express `usesCleartextTraffic`.
This experimental APK therefore targets API 27 to permit LAN HTTP by default,
while compiling Slint's Java helper against SDK 35. This is not a Play Store
release configuration. HTTPS is also supported.

Host regression checks:

```powershell
cargo check -p lb-gui
cargo test -p lb-gui --lib
```

For runtime verification, install on a device/emulator, check the form with an
unreachable URL and an invalid token, connect to a working server, navigate
screens, reopen the app, and verify that only the URL was remembered.

//! Is FFmpeg available? The application needs `ffmpeg` and `ffprobe`.

use std::process::{Command, Stdio};

/// Version of the `ffmpeg` found on the PATH.
pub fn detect() -> Option<String> {
    if crate::payload::has_ffmpeg() {
        return Some("в комплекте установщика".into());
    }
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-version").stdin(Stdio::null()).stderr(Stdio::null());
    crate::platform::no_window(&mut cmd);
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?;
    // "ffmpeg version 7.1-full_build-www.gyan.dev Copyright ..." → "7.1-full_build-www.gyan.dev"
    Some(line.split_whitespace().nth(2).unwrap_or(line).to_string())
}

/// How to install FFmpeg on this system.
pub fn install_hint() -> &'static str {
    if cfg!(windows) { "winget install Gyan.FFmpeg" } else { "sudo apt install ffmpeg" }
}

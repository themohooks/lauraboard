//! Hardware encoder detection: an encoder is usable only if a short test
//! encode succeeds (being listed by `-encoders` is not enough).

use std::time::Duration;

use crate::process::command;

/// Preference order for `video_codec = "auto"`.
pub const H264_CANDIDATES: &[&str] = &["h264_nvenc", "h264_qsv", "h264_amf", "h264_videotoolbox", "h264_vaapi", "libx264"];

/// Try a 0.2 s encode with the given codec.
pub async fn encoder_works(ffmpeg: &str, codec: &str) -> bool {
    let mut cmd = command(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    if codec.ends_with("_vaapi") {
        cmd.args(["-vaapi_device", "/dev/dri/renderD128"]);
    }
    cmd.args(["-f", "lavfi", "-i", "color=s=320x240:r=25:d=0.2"]);
    if codec.ends_with("_vaapi") {
        cmd.args(["-vf", "format=nv12,hwupload"]);
    } else {
        cmd.args(["-pix_fmt", "yuv420p"]);
    }
    cmd.args(["-c:v", codec, "-f", "null", "-"]);
    match tokio::time::timeout(Duration::from_secs(15), cmd.output()).await {
        Ok(Ok(out)) => out.status.success(),
        _ => false,
    }
}

/// Resolve `auto` to the first working codec; other values pass through.
pub async fn resolve_codec(ffmpeg: &str, configured: &str) -> String {
    if configured != "auto" {
        return configured.to_string();
    }
    for c in H264_CANDIDATES {
        if encoder_works(ffmpeg, c).await {
            tracing::info!(codec = c, "selected video encoder");
            return c.to_string();
        }
    }
    "libx264".into()
}

/// FFmpeg version string, or `None` when ffmpeg cannot be executed.
pub async fn ffmpeg_version(ffmpeg: &str) -> Option<String> {
    let out = command(ffmpeg).arg("-version").output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or_default().to_string())
}

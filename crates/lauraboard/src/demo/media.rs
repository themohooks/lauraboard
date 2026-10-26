//! Test clips generated with FFmpeg's lavfi sources (no fonts needed).

use std::path::Path;
use std::process::Command;

/// (file stem, seconds, video source, tone Hz)
pub const CLIPS: &[(&str, u32, &str, u32)] = &[
    ("show-1", 90, "testsrc2", 440),
    ("show-2", 75, "smptebars", 523),
    ("show-3", 60, "rgbtestsrc", 587),
    ("show-4", 80, "testsrc", 659),
    ("movie-1", 240, "testsrc2", 330),
    ("news", 30, "smptehdbars", 880),
    ("jingle-1", 5, "testsrc2", 1000),
    ("jingle-2", 6, "pal100bars", 1200),
    ("ad-1", 15, "yuvtestsrc", 700),
    ("ad-2", 20, "pal75bars", 750),
    ("ad-3", 30, "smptebars", 800),
    ("promo-movie", 10, "zoneplate", 900),
    ("filler-1", 30, "rgbtestsrc", 300),
    ("filler-2", 45, "testsrc", 350),
];

fn source(name: &str, secs: u32) -> String {
    format!("{name}=s=1280x720:r=25:d={secs}")
}

/// Generate all clips into `dir`. Returns false when FFmpeg is missing.
pub fn generate(ffmpeg: &str, dir: &Path) -> bool {
    let ffmpeg = lb_media::process::bundled_program(ffmpeg);
    let ffmpeg = &ffmpeg;
    if Command::new(ffmpeg).arg("-version").output().is_err() {
        return false;
    }
    for (stem, secs, src, hz) in CLIPS {
        let out = dir.join(format!("{stem}.mp4"));
        if out.exists() {
            continue;
        }
        eprintln!("generating {}", out.display());
        let status = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg(source(src, *secs))
            .args(["-f", "lavfi", "-i"])
            .arg(format!("sine=frequency={hz}:sample_rate=48000:duration={secs}"))
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "32", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(&out)
            .status();
        if !status.is_ok_and(|s| s.success()) {
            eprintln!("warning: failed to generate {stem} (source {src})");
        }
    }
    true
}

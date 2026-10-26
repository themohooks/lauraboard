//! Pure builders for ffmpeg command lines (no process spawning here, so
//! they are easy to unit test).
//!
//! * [`filters`] — scaling / padding / fades / audio normalisation chains
//! * [`decoder`] — one rundown item → normalised MPEG-TS on stdout
//! * [`slate`] — generated standby picture with silence
//! * [`scene`] — scene: background with music tracks
//! * [`encoder`] — overlay graphics, encode and push to outputs
//! * [`outputs`] — output target URL → muxer options (tee syntax)
//! * [`record`] — recorder: files from the encoder's local feed
//! * [`color`] — colour string conversion
//! * [`loops`] — ready-made seamless background videos

pub mod color;
pub mod decoder;
pub mod encoder;
pub mod filters;
pub mod loops;
pub mod outputs;
pub mod record;
pub mod scene;
pub mod slate;

/// Intermediate format between decoders and the encoder: MPEG-TS with
/// high-quality MPEG-2 video and PCM audio (SMPTE 302M). PCM keeps audio
/// sample-exact. The encoder follows the shared MPEG-TS timestamps and
/// compensates gaps instead of assuming equal audio and video lengths.
pub fn intermediate_codec_args(fps: f64, audio_channels: u32) -> Vec<String> {
    let gop = (fps.round() as i64 / 2).max(1);
    let mut a = sv(&["-c:v", "mpeg2video", "-q:v", "2", "-g", &gop.to_string(), "-bf", "0"]);
    if audio_channels >= 2 && audio_channels.is_multiple_of(2) {
        a.extend(sv(&["-c:a", "s302m", "-strict", "-2"]));
    } else {
        a.extend(sv(&["-c:a", "mp2", "-b:a", "256k"]));
    }
    a.extend(sv(&["-f", "mpegts", "-muxdelay", "0", "-muxpreload", "0", "pipe:1"]));
    a
}

/// Round a duration to a whole number of frames.
pub fn frame_align_ms(ms: i64, fps: f64) -> i64 {
    if fps <= 0.0 {
        return ms;
    }
    let frames = (ms as f64 * fps / 1000.0).round().max(1.0);
    (frames * 1000.0 / fps).round() as i64
}

/// Common leading arguments.
pub fn base_args(loglevel: &str) -> Vec<String> {
    sv(&["-hide_banner", "-nostdin", "-loglevel", loglevel])
}

/// `&[&str]` → `Vec<String>`.
pub fn sv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// Seconds with millisecond precision for ffmpeg time options.
pub fn secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// Frame rate as ffmpeg-friendly string (`25`, `29.97`).
pub fn fps_str(fps: f64) -> String {
    if (fps - fps.round()).abs() < 1e-6 { format!("{}", fps.round() as i64) } else { format!("{fps:.3}") }
}

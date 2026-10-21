//! Pure builders for ffmpeg command lines (no process spawning here, so
//! they are easy to unit test).
//!
//! * [`filters`] — scaling / padding / fades / audio normalisation chains
//! * [`decoder`] — one rundown item → normalised MPEG-TS on stdout
//! * [`slate`] — generated standby picture with silence
//! * [`encoder`] — overlay graphics, encode and push to outputs
//! * [`outputs`] — output target URL → muxer options (tee syntax)
//! * [`color`] — colour string conversion

pub mod color;
pub mod decoder;
pub mod encoder;
pub mod filters;
pub mod outputs;
pub mod slate;

/// Intermediate format between decoders and the encoder: MPEG-TS with
/// near-lossless MPEG-2 intra-ish video and MP2 audio. Cheap to produce and
/// tolerant to concatenation (timestamp discontinuities are handled by
/// ffmpeg's mpegts demuxer).
pub fn intermediate_codec_args(fps: f64) -> Vec<String> {
    let gop = (fps.round() as i64 / 2).max(1);
    sv(&[
        "-c:v",
        "mpeg2video",
        "-q:v",
        "2",
        "-g",
        &gop.to_string(),
        "-bf",
        "0",
        "-c:a",
        "mp2",
        "-b:a",
        "384k",
        "-f",
        "mpegts",
        "-muxdelay",
        "0",
        "-muxpreload",
        "0",
        "pipe:1",
    ])
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

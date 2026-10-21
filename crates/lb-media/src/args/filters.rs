//! Filter chains for normalising any source to the channel format.

use lb_core::config::{AudioFormat, ScaleMode, VideoFormat};

use super::color::ffmpeg_color;
use super::{fps_str, secs};

/// Video normalisation: size, aspect handling, SAR, fps, pixel format,
/// optional fades (relative to the item start, duration `dur_ms`).
pub fn video_chain(v: &VideoFormat, fade_in_ms: i64, fade_out_ms: i64, dur_ms: i64) -> String {
    let (w, h) = (v.width, v.height);
    let mut f: Vec<String> = Vec::new();
    match v.scale {
        ScaleMode::Fit => {
            f.push(format!("scale={w}:{h}:force_original_aspect_ratio=decrease"));
            f.push(format!("pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color={}", ffmpeg_color(&v.background)));
        }
        ScaleMode::Fill => {
            f.push(format!("scale={w}:{h}:force_original_aspect_ratio=increase"));
            f.push(format!("crop={w}:{h}"));
        }
        ScaleMode::Stretch => f.push(format!("scale={w}:{h}")),
    }
    f.push("setsar=1".into());
    f.push(format!("fps={}", fps_str(v.fps)));
    f.push("format=yuv420p".into());
    if fade_in_ms > 0 {
        f.push(format!("fade=t=in:st=0:d={}", secs(fade_in_ms)));
    }
    if fade_out_ms > 0 && dur_ms > fade_out_ms {
        f.push(format!("fade=t=out:st={}:d={}", secs(dur_ms - fade_out_ms), secs(fade_out_ms)));
    }
    f.join(",")
}

/// Audio normalisation: resample, layout, gain, loudness, fades, padding
/// with silence so audio always covers the item duration.
pub fn audio_chain(a: &AudioFormat, volume_db: f32, fade_in_ms: i64, fade_out_ms: i64, dur_ms: i64) -> String {
    let layout = match a.channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n}c"),
    };
    let mut f: Vec<String> = vec![format!("aresample={}", a.sample_rate)];
    if a.normalize {
        f.push("loudnorm=I=-23:TP=-2:LRA=11".into());
        f.push(format!("aresample={}", a.sample_rate));
    }
    f.push(format!("aformat=sample_fmts=s16:channel_layouts={layout}"));
    if volume_db.abs() > 0.01 {
        f.push(format!("volume={volume_db:.2}dB"));
    }
    if fade_in_ms > 0 {
        f.push(format!("afade=t=in:st=0:d={}", secs(fade_in_ms)));
    }
    if fade_out_ms > 0 && dur_ms > fade_out_ms {
        f.push(format!("afade=t=out:st={}:d={}", secs(dur_ms - fade_out_ms), secs(fade_out_ms)));
    }
    f.push("apad".into());
    f.join(",")
}

/// `anullsrc` source matching the channel audio format.
pub fn silence_src(a: &AudioFormat) -> String {
    let layout = if a.channels == 1 { "mono" } else { "stereo" };
    format!("anullsrc=r={}:cl={layout}", a.sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_chain_with_fades() {
        let v = VideoFormat::default();
        let c = video_chain(&v, 500, 1000, 10_000);
        assert!(c.starts_with("scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720"));
        assert!(c.contains("fps=25"));
        assert!(c.contains("fade=t=in:st=0:d=0.500"));
        assert!(c.contains("fade=t=out:st=9.000:d=1.000"));
    }

    #[test]
    fn audio_chain_pads() {
        let a = AudioFormat::default();
        let c = audio_chain(&a, -3.0, 0, 0, 1000);
        assert!(c.contains("aformat=sample_fmts=s16:channel_layouts=stereo"));
        assert!(c.contains("volume=-3.00dB"));
        assert!(c.ends_with("apad"));
    }
}

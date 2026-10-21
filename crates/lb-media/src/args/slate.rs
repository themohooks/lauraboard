//! Slate: generated standby picture (colour or image) with silence, in the
//! intermediate format. Used for gaps, missing media and as the last resort.

use lb_core::config::{AudioFormat, VideoFormat};

use super::color::ffmpeg_color;
use super::filters::{silence_src, video_chain};
use super::{base_args, fps_str, frame_align_ms, intermediate_codec_args, secs, sv};

/// `image` is a resolved path; `None` → solid background colour.
pub fn slate_args(image: Option<&str>, duration_ms: i64, v: &VideoFormat, a: &AudioFormat, loglevel: &str) -> Vec<String> {
    let mut args = base_args(loglevel);
    match image {
        Some(img) => args.extend(sv(&["-re", "-readrate_initial_burst", "0", "-loop", "1", "-framerate", &fps_str(v.fps), "-i", img])),
        None => {
            let src = format!("color=c={}:s={}x{}:r={}", ffmpeg_color(&v.background), v.width, v.height, fps_str(v.fps));
            args.extend(sv(&["-re", "-readrate_initial_burst", "0", "-f", "lavfi", "-i", &src]));
        }
    }
    args.extend(sv(&["-f", "lavfi", "-i", &silence_src(a)]));
    let fc = format!("[0:v:0]{}[v]", video_chain(v, 0, 0, duration_ms));
    args.extend(sv(&["-filter_complex", &fc, "-map", "[v]", "-map", "1:a:0"]));
    args.extend(sv(&["-ar", &a.sample_rate.to_string(), "-ac", &a.channels.to_string()]));
    if duration_ms > 0 {
        args.extend(sv(&["-t", &secs(frame_align_ms(duration_ms, v.fps))]));
    }
    args.extend(intermediate_codec_args(v.fps, a.channels));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_slate() {
        let s = slate_args(None, 5000, &VideoFormat::default(), &AudioFormat::default(), "error").join(" ");
        assert!(s.contains("-f lavfi -i color=c=0x000000:s=1280x720:r=25"));
        assert!(s.contains("-t 5.000"));
    }

    #[test]
    fn image_slate() {
        let s = slate_args(Some("standby.png"), 0, &VideoFormat::default(), &AudioFormat::default(), "error").join(" ");
        assert!(s.contains("-re -readrate_initial_burst 0 -loop 1 -framerate 25 -i standby.png"));
        assert!(!s.contains("-t "));
    }
}

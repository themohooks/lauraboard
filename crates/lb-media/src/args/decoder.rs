//! Decoder: one rundown item → normalised MPEG-TS on stdout.

use lb_core::config::{AudioFormat, VideoFormat};

use super::filters::{audio_chain, silence_src, video_chain};
use super::{base_args, fps_str, intermediate_codec_args, secs, sv};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// Regular media file.
    File,
    /// Still image, looped for the duration.
    Image,
    /// Network stream (no seeking, reconnect options).
    Stream,
}

/// Everything needed to decode one item.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeJob {
    /// Resolved path or URL.
    pub input: String,
    pub kind: InputKind,
    /// Start offset into the media.
    pub seek_ms: i64,
    /// How long to play (planned duration minus already played part).
    pub duration_ms: i64,
    pub has_audio: bool,
    pub volume_db: f32,
    pub fade_in_ms: i64,
    pub fade_out_ms: i64,
}

/// Build decoder arguments (without the program name).
pub fn decoder_args(job: &DecodeJob, v: &VideoFormat, a: &AudioFormat, loglevel: &str) -> Vec<String> {
    let mut args = base_args(loglevel);
    match job.kind {
        InputKind::File => {
            if job.seek_ms > 0 {
                args.extend(sv(&["-ss", &secs(job.seek_ms)]));
            }
            args.extend(sv(&["-i", &job.input]));
        }
        InputKind::Image => {
            args.extend(sv(&["-loop", "1", "-framerate", &fps_str(v.fps), "-i", &job.input]));
        }
        InputKind::Stream => {
            if job.input.starts_with("http") {
                args.extend(sv(&["-reconnect", "1", "-reconnect_streamed", "1", "-reconnect_delay_max", "5"]));
            }
            args.extend(sv(&["-rw_timeout", "10000000", "-i", &job.input]));
        }
    }
    let audio_from_input = job.has_audio && job.kind != InputKind::Image;
    if !audio_from_input {
        args.extend(sv(&["-f", "lavfi", "-i", &silence_src(a)]));
    }
    let vf = video_chain(v, job.fade_in_ms, job.fade_out_ms, job.duration_ms);
    let af = audio_chain(a, job.volume_db, job.fade_in_ms, job.fade_out_ms, job.duration_ms);
    let a_in = if audio_from_input { "0:a:0" } else { "1:a:0" };
    let fc = format!("[0:v:0]{vf}[v];[{a_in}]{af}[a]");
    args.extend(sv(&["-filter_complex", &fc, "-map", "[v]", "-map", "[a]"]));
    if job.duration_ms > 0 {
        args.extend(sv(&["-t", &secs(job.duration_ms)]));
    }
    args.extend(intermediate_codec_args(v.fps));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(kind: InputKind, has_audio: bool) -> DecodeJob {
        DecodeJob {
            input: "media/a.mp4".into(),
            kind,
            seek_ms: 1500,
            duration_ms: 60_000,
            has_audio,
            volume_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
        }
    }

    fn joined(j: &DecodeJob) -> String {
        decoder_args(j, &VideoFormat::default(), &AudioFormat::default(), "warning").join(" ")
    }

    #[test]
    fn file_with_seek_and_audio() {
        let s = joined(&job(InputKind::File, true));
        assert!(s.contains("-ss 1.500 -i media/a.mp4"));
        assert!(s.contains("[0:a:0]aresample=48000"));
        assert!(!s.contains("anullsrc"));
        assert!(s.contains("-t 60.000"));
        assert!(s.ends_with("-f mpegts -muxdelay 0 -muxpreload 0 pipe:1"));
    }

    #[test]
    fn silent_file_gets_generated_audio() {
        let s = joined(&job(InputKind::File, false));
        assert!(s.contains("-f lavfi -i anullsrc=r=48000:cl=stereo"));
        assert!(s.contains("[1:a:0]"));
    }

    #[test]
    fn image_is_looped() {
        let s = joined(&job(InputKind::Image, true));
        assert!(s.contains("-loop 1 -framerate 25 -i media/a.mp4"));
        assert!(!s.contains("-ss"));
        assert!(s.contains("anullsrc"));
    }
}

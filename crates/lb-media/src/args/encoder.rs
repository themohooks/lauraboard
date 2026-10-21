//! Encoder: reads the intermediate stream on stdin, overlays graphics,
//! encodes and pushes to all outputs; writes an MJPEG preview to stdout.

use lb_core::config::{AudioFormat, EncoderConfig, OutputTarget, RecordConfig, VideoFormat};

use super::outputs::{record_slave, tee_slave};
use super::{base_args, fps_str, sv};

/// Graphics overlay input (raw RGBA frames over TCP from the engine).
#[derive(Debug, Clone, PartialEq)]
pub struct GfxInput {
    pub port: u16,
    pub fps: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewOut {
    pub width: u32,
    pub fps: f64,
}

#[derive(Debug, Clone)]
pub struct EncodeJob<'a> {
    pub channel: &'a str,
    pub video: &'a VideoFormat,
    pub audio: &'a AudioFormat,
    pub encoder: &'a EncoderConfig,
    /// Concrete video codec (after resolving `auto`).
    pub video_codec: &'a str,
    pub outputs: &'a [OutputTarget],
    /// Recording with its resolved directory.
    pub record: Option<(&'a RecordConfig, String)>,
    pub gfx: Option<GfxInput>,
    pub preview: Option<PreviewOut>,
    pub loglevel: &'a str,
}

pub fn encoder_args(j: &EncodeJob) -> Vec<String> {
    let v = j.video;
    let mut args = base_args(j.loglevel);
    args.retain(|a| a != "-nostdin"); // stdin carries the stream
    args.extend(sv(&["-nostats", "-progress", "pipe:2", "-stats_period", "1"]));
    if j.video_codec.ends_with("_vaapi") {
        args.extend(sv(&["-vaapi_device", "/dev/dri/renderD128"]));
    }
    // input 0: intermediate stream (already paced by the decoders)
    args.extend(sv(&["-fflags", "+genpts+discardcorrupt", "-f", "mpegts", "-i", "pipe:0"]));
    // input 1: graphics
    if let Some(g) = &j.gfx {
        args.extend(sv(&[
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-video_size",
            &format!("{}x{}", v.width, v.height),
            "-framerate",
            &fps_str(g.fps),
            "-i",
            &format!("tcp://127.0.0.1:{}", g.port),
        ]));
    }
    args.extend(sv(&["-filter_complex", &filter_graph(j)]));
    args.extend(sv(&["-map", "[vout]", "-map", "[aout]"]));
    args.extend(video_codec_args(j));
    args.extend(sv(&[
        "-c:a",
        &j.encoder.audio_codec,
        "-b:a",
        &j.encoder.audio_bitrate,
        "-ar",
        &j.audio.sample_rate.to_string(),
        "-ac",
        &j.audio.channels.to_string(),
    ]));
    args.extend(j.encoder.extra_args.iter().cloned());
    args.extend(sv(&["-flags", "+global_header", "-f", "tee", &tee_spec(j)]));
    if let Some(p) = &j.preview {
        let _ = p;
        args.extend(sv(&["-map", "[pv]", "-an", "-c:v", "mjpeg", "-q:v", "6", "-f", "image2pipe", "pipe:1"]));
    }
    args
}

fn filter_graph(j: &EncodeJob) -> String {
    let mut parts: Vec<String> = Vec::new();
    let vbase = if j.gfx.is_some() {
        parts.push("[0:v][1:v]overlay=x=0:y=0:eof_action=pass:repeatlast=1:format=auto:alpha=premultiplied,format=yuv420p[vmix]".into());
        "[vmix]"
    } else {
        parts.push("[0:v]format=yuv420p[vmix]".into());
        "[vmix]"
    };
    let hw_tail = if j.video_codec.ends_with("_vaapi") { ",format=nv12,hwupload" } else { "" };
    match &j.preview {
        Some(p) => {
            parts.push(format!("{vbase}split=2[venc][vprev]"));
            parts.push(format!("[venc]null{hw_tail}[vout]"));
            parts.push(format!("[vprev]fps={},scale={}:-2[pv]", fps_str(p.fps), p.width));
        }
        None => parts.push(format!("{vbase}null{hw_tail}[vout]")),
    }
    let gain = if j.audio.volume_db.abs() > 0.01 { format!("volume={:.2}dB,", j.audio.volume_db) } else { String::new() };
    // async resampling keeps A/V in sync across item boundaries
    parts.push(format!("[0:a]{gain}aresample=async=1000:first_pts=0[aout]"));
    parts.join(";")
}

fn video_codec_args(j: &EncodeJob) -> Vec<String> {
    let e = j.encoder;
    let codec = j.video_codec;
    let gop = ((j.video.fps * e.gop_seconds).round() as i64).max(1).to_string();
    let mut a = sv(&["-c:v", codec]);
    if codec.contains("nvenc") {
        let preset = if e.preset.starts_with('p') && e.preset.len() == 2 { e.preset.clone() } else { "p4".into() };
        a.extend(sv(&["-preset", &preset, "-rc", "cbr"]));
    } else if codec.contains("amf") {
        a.extend(sv(&["-quality", "speed", "-rc", "cbr"]));
    } else if codec.contains("qsv") || codec == "libx264" || codec == "libx265" {
        a.extend(sv(&["-preset", &e.preset]));
        if codec.starts_with("libx26") {
            a.extend(sv(&["-tune", "zerolatency"]));
        }
    }
    a.extend(sv(&["-b:v", &e.video_bitrate]));
    let maxrate = e.maxrate.clone().unwrap_or_else(|| e.video_bitrate.clone());
    let bufsize = e.bufsize.clone().unwrap_or_else(|| double_rate(&maxrate));
    a.extend(sv(&["-maxrate", &maxrate, "-bufsize", &bufsize, "-g", &gop, "-keyint_min", &gop]));
    if codec.starts_with("libx26") {
        a.extend(sv(&["-sc_threshold", "0"]));
    }
    if !codec.ends_with("_vaapi") {
        a.extend(sv(&["-pix_fmt", "yuv420p"]));
    }
    a
}

/// "3000k" → "6000k" (bufsize default = 2 × maxrate).
fn double_rate(r: &str) -> String {
    let (num, unit) = r.trim().split_at(r.trim().find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(r.trim().len()));
    match num.parse::<f64>() {
        Ok(n) => format!("{}{unit}", (n * 2.0).round() as i64),
        Err(_) => r.to_string(),
    }
}

fn tee_spec(j: &EncodeJob) -> String {
    let mut slaves: Vec<String> = j.outputs.iter().filter(|o| o.enabled).map(tee_slave).collect();
    if let Some((rec, dir)) = &j.record {
        slaves.push(record_slave(rec, dir, j.channel));
    }
    if slaves.is_empty() {
        // nothing configured: still run the pipeline (preview, monitoring)
        slaves.push("[f=null]-".into());
    }
    slaves.join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job<'a>(e: &'a EncoderConfig, v: &'a VideoFormat, a: &'a AudioFormat, outs: &'a [OutputTarget]) -> EncodeJob<'a> {
        EncodeJob {
            channel: "main",
            video: v,
            audio: a,
            encoder: e,
            video_codec: &e.video_codec,
            outputs: outs,
            record: None,
            gfx: Some(GfxInput { port: 40000, fps: 10.0 }),
            preview: Some(PreviewOut { width: 480, fps: 5.0 }),
            loglevel: "warning",
        }
    }

    #[test]
    fn full_pipeline() {
        let (e, v, a) = (EncoderConfig::default(), VideoFormat::default(), AudioFormat::default());
        let outs = vec![OutputTarget { name: "yt".into(), url: "rtmp://x/live/key".into(), enabled: true, format: None }];
        let s = encoder_args(&job(&e, &v, &a, &outs)).join(" ");
        assert!(s.contains("-fflags +genpts+discardcorrupt -f mpegts -i pipe:0"));
        assert!(!s.contains("-re "));
        assert!(s.contains("-i tcp://127.0.0.1:40000"));
        assert!(s.contains("overlay=x=0:y=0"));
        assert!(s.contains("[vprev]fps=5,scale=480:-2[pv]"));
        assert!(s.contains("-c:v libx264 -preset veryfast -tune zerolatency -b:v 3000k -maxrate 3000k -bufsize 6000k -g 50"));
        assert!(s.contains("-f tee [f=flv:onfail=abort]rtmp://x/live/key"));
        assert!(s.ends_with("-f image2pipe pipe:1"));
        assert!(!s.contains("-nostdin"));
    }

    #[test]
    fn nvenc_and_no_outputs() {
        let e = EncoderConfig { video_codec: "h264_nvenc".into(), ..Default::default() };
        let (v, a) = (VideoFormat::default(), AudioFormat::default());
        let mut j = job(&e, &v, &a, &[]);
        j.gfx = None;
        j.preview = None;
        let s = encoder_args(&j).join(" ");
        assert!(s.contains("-c:v h264_nvenc -preset p4 -rc cbr"));
        assert!(s.contains("[0:v]format=yuv420p[vmix]"));
        assert!(s.contains("-f tee [f=null]-"));
        assert_eq!(double_rate("2.5M"), "5M");
    }
}

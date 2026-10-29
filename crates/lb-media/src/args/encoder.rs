//! Encoder: reads the intermediate stream on stdin, overlays graphics,
//! encodes and pushes to all outputs; writes an MJPEG preview to stdout.

use lb_core::config::{AudioFormat, EncoderConfig, OutputTarget, RateControl, VideoFormat};

use super::filters::sync_resample;
use super::outputs::{record_feed_slave, tee_slave};
use super::{base_args, fps_str, sv};

/// Graphics overlay input (raw RGBA frames over TCP from the engine).
#[derive(Debug, Clone, PartialEq)]
pub struct GfxInput {
    pub port: u16,
    pub audio_port: u16,
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
    /// Local port of the recorder: the encoder feeds it a copy of the
    /// output (always while it runs, so recording settings never need an
    /// encoder restart).
    pub record_feed: Option<u16>,
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
    // items are normalised identically: never rebuild the filter graph on a
    // stream change (that would reset the synchronisation filters)
    args.extend(sv(&["-fflags", "+genpts+discardcorrupt", "-reinit_filter", "0", "-f", "mpegts", "-i", "pipe:0"]));
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
    if let Some(g) = &j.gfx {
        args.extend(sv(&[
            "-thread_queue_size",
            "512",
            "-f",
            "f32le",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-i",
            &format!("tcp://127.0.0.1:{}", g.audio_port),
        ]));
    }
    args.extend(sv(&["-filter_complex", &filter_graph(j)]));
    args.extend(sv(&["-map", "[vout]", "-map", "[aout]"]));
    args.extend(video_codec_args(j));
    // Main output only: the preview has its own frame rate and must never
    // contribute bytes or synchronization losses to broadcast statistics.
    args.extend(sv(&[
        "-stats_enc_pre:v:0",
        "pipe:2",
        "-stats_enc_pre_fmt:v:0",
        "LB_FRAME {ni}",
        "-stats_enc_post",
        "pipe:2",
        "-stats_enc_post_fmt",
        "LB_PACKET {sidx} {size}",
    ]));
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
    // Keep the shared timeline repaired by the MPEG-TS demuxer at item
    // boundaries. Counting video frames and audio samples independently
    // turns every lost frame or partial audio packet into permanent drift.
    let fps = fps_str(j.video.fps);
    parts.push(format!("[0:v]fps={fps}:start_time=0[vin]"));
    let vbase = if j.gfx.is_some() {
        parts.push("[vin][1:v]overlay=x=0:y=0:eof_action=pass:repeatlast=1:format=auto,format=yuv420p[vmix]".into());
        "[vmix]"
    } else {
        parts.push("[vin]format=yuv420p[vmix]".into());
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
    // Resample against timestamps BEFORE any sample-count timestamp reset.
    // Fill/trim gaps and gently compensate clock drift on long broadcasts.
    if j.gfx.is_some() {
        parts.push(format!("[0:a]{}[amain]", sync_resample(j.audio.sample_rate)));
        parts.push(format!("[2:a]aresample={}[agfx]", j.audio.sample_rate));
        parts.push(format!("[amain][agfx]amix=inputs=2:duration=first:dropout_transition=0:normalize=0,{gain}anull[aout]"));
    } else {
        parts.push(format!("[0:a]{},{gain}anull[aout]", sync_resample(j.audio.sample_rate)));
    }
    parts.join(";")
}

fn video_codec_args(j: &EncodeJob) -> Vec<String> {
    let e = j.encoder;
    let codec = j.video_codec;
    let x26 = codec.starts_with("libx26");
    let (nvenc, amf, qsv, vaapi) = (codec.contains("nvenc"), codec.contains("amf"), codec.contains("qsv"), codec.ends_with("_vaapi"));
    let gop = ((j.video.fps * e.gop_seconds).round() as i64).max(1).to_string();
    let mut a = sv(&["-c:v", codec]);

    // speed / quality preset
    if nvenc {
        let preset = if e.preset.starts_with('p') && e.preset.len() == 2 { e.preset.clone() } else { "p4".into() };
        a.extend(sv(&["-preset", &preset]));
    } else if amf {
        let quality = if matches!(e.preset.as_str(), "speed" | "balanced" | "quality") { e.preset.as_str() } else { "speed" };
        a.extend(sv(&["-quality", quality]));
    } else if qsv || x26 {
        a.extend(sv(&["-preset", &e.preset]));
    }
    if x26 && !e.tune.trim().is_empty() {
        a.extend(sv(&["-tune", e.tune.trim()]));
    }
    if !e.profile.trim().is_empty() && !vaapi {
        a.extend(sv(&["-profile:v", e.profile.trim()]));
    }
    if !e.level.trim().is_empty() {
        a.extend(sv(&["-level", e.level.trim()]));
    }

    // rate control
    let maxrate = e.maxrate.clone().filter(|m| !m.trim().is_empty());
    let crf = e.crf.min(51).to_string();
    match e.rate_control {
        RateControl::Cbr => {
            if nvenc || amf {
                a.extend(sv(&["-rc", "cbr"]));
            }
            a.extend(sv(&["-b:v", &e.video_bitrate]));
            let maxrate = maxrate.unwrap_or_else(|| e.video_bitrate.clone());
            let bufsize = e.bufsize.clone().unwrap_or_else(|| double_rate(&maxrate));
            a.extend(sv(&["-maxrate", &maxrate, "-bufsize", &bufsize]));
        }
        RateControl::Vbr => {
            if nvenc {
                a.extend(sv(&["-rc", "vbr"]));
            } else if amf {
                a.extend(sv(&["-rc", "vbr_peak"]));
            }
            a.extend(sv(&["-b:v", &e.video_bitrate]));
            let maxrate = maxrate.unwrap_or_else(|| scale_rate(&e.video_bitrate, 1.5));
            let bufsize = e.bufsize.clone().unwrap_or_else(|| double_rate(&maxrate));
            a.extend(sv(&["-maxrate", &maxrate, "-bufsize", &bufsize]));
        }
        RateControl::Quality => {
            if nvenc {
                a.extend(sv(&["-rc", "vbr", "-cq", &crf, "-b:v", "0"]));
            } else if amf {
                a.extend(sv(&["-rc", "cqp", "-qp_i", &crf, "-qp_p", &crf, "-qp_b", &crf]));
            } else if qsv {
                a.extend(sv(&["-global_quality", &crf]));
            } else if vaapi {
                a.extend(sv(&["-rc_mode", "CQP", "-qp", &crf]));
            } else if codec.ends_with("_videotoolbox") {
                // VideoToolbox uses 1…100, higher is better; it has no CRF.
                let quality = (100.0 - e.crf.min(51) as f64 * 99.0 / 51.0).round().to_string();
                a.extend(sv(&["-q:v", &quality]));
            } else {
                a.extend(sv(&["-crf", &crf]));
            }
            // capped: a quality target must not flood the uplink
            if let Some(m) = maxrate {
                let bufsize = e.bufsize.clone().unwrap_or_else(|| double_rate(&m));
                a.extend(sv(&["-maxrate", &m, "-bufsize", &bufsize]));
            }
        }
    }

    // frame structure
    if e.scene_cut {
        a.extend(sv(&["-g", &gop]));
    } else {
        a.extend(sv(&["-g", &gop, "-keyint_min", &gop]));
        if x26 {
            a.extend(sv(&["-sc_threshold", "0"]));
        } else if nvenc {
            a.extend(sv(&["-no-scenecut", "1"]));
        }
    }
    if let Some(bf) = e.bframes {
        a.extend(sv(&["-bf", &bf.min(16).to_string()]));
    }
    if e.threads > 0 {
        a.extend(sv(&["-threads", &e.threads.to_string()]));
    }
    if !vaapi {
        let pix = if e.pix_fmt.trim().is_empty() { "yuv420p" } else { e.pix_fmt.trim() };
        a.extend(sv(&["-pix_fmt", pix]));
    }
    a
}

/// "3000k" × 1.5 → "4500k".
fn scale_rate(r: &str, k: f64) -> String {
    let r = r.trim();
    let (num, unit) = r.split_at(r.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(r.len()));
    match num.parse::<f64>() {
        Ok(n) => format!("{}{unit}", (n * k).round() as i64),
        Err(_) => r.to_string(),
    }
}

/// "3000k" → "6000k" (bufsize default = 2 × maxrate).
fn double_rate(r: &str) -> String {
    scale_rate(r, 2.0)
}

fn tee_spec(j: &EncodeJob) -> String {
    let mut slaves: Vec<String> = j.outputs.iter().filter(|o| o.enabled).map(tee_slave).collect();
    if let Some(port) = j.record_feed {
        slaves.push(record_feed_slave(port));
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

    #[tokio::test]
    #[ignore = "requires FFmpeg with libx264"]
    async fn live_telemetry_counts_filter_drops_and_excludes_preview_packets() {
        let e = EncoderConfig::default();
        let v = VideoFormat { width: 160, height: 90, fps: 10.0, ..Default::default() };
        let a = AudioFormat::default();
        let mut j = job(&e, &v, &a, &[]);
        j.gfx = None;
        j.preview = Some(PreviewOut { width: 80, fps: 5.0 });
        let mut args = encoder_args(&j);
        let first = args.iter().position(|s| s == "-fflags").unwrap();
        let last = args.iter().position(|s| s == "pipe:0").unwrap() + 1;
        // Deliberately reduce 25 input fps to 10 before encoding. These
        // losses are invisible to FFmpeg's aggregate drop_frames counter.
        args.splice(
            first..last,
            ["-t", "2", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=25[out0];sine=sample_rate=48000[out1]"].map(str::to_string),
        );
        let output = crate::process::command("ffmpeg").args(args).output().await.unwrap();
        let log = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{log}");
        assert!(log.contains("bitrate=N/A"), "tee progress should reproduce the missing bitrate: {log}");
        let mut p = crate::progress::ProgressParser::default();
        for line in log.lines() {
            p.feed(line);
        }
        let stats = p.sample(std::time::Duration::from_secs(2), std::time::Duration::from_secs(2));
        assert!(stats.bitrate_kbps > 0.0);
        assert_eq!(stats.fps, 10.0, "preview packets must not add 5 fps to broadcast statistics");
        assert!(stats.sync_drop_frames >= 25, "fps filter losses were not counted: {stats:?}");
        assert_eq!(stats.sync_dup_frames, 0);
        assert_eq!(stats.drop_frames, 0, "this case must reproduce hidden filter drops");
    }

    fn job<'a>(e: &'a EncoderConfig, v: &'a VideoFormat, a: &'a AudioFormat, outs: &'a [OutputTarget]) -> EncodeJob<'a> {
        EncodeJob {
            channel: "main",
            video: v,
            audio: a,
            encoder: e,
            video_codec: &e.video_codec,
            outputs: outs,
            record_feed: None,
            gfx: Some(GfxInput { port: 40000, fps: 10.0, audio_port: 12346 }),
            preview: Some(PreviewOut { width: 480, fps: 5.0 }),
            loglevel: "warning",
        }
    }

    #[test]
    fn videotoolbox_quality_uses_apple_scale_without_x264_options() {
        let e = EncoderConfig { rate_control: RateControl::Quality, ..Default::default() };
        let (v, a) = (VideoFormat::default(), AudioFormat::default());
        let mut j = job(&e, &v, &a, &[]);
        j.video_codec = "h264_videotoolbox";
        let s = encoder_args(&j).join(" ");
        assert!(s.contains("-q:v "));
        assert!(!s.contains("-crf") && !s.contains("-preset") && !s.contains("-tune"));
    }

    #[test]
    fn full_pipeline() {
        let (e, v, a) = (EncoderConfig::default(), VideoFormat::default(), AudioFormat::default());
        let outs = vec![OutputTarget { name: "yt".into(), url: "rtmp://x/live/key".into(), enabled: true, format: None }];
        let s = encoder_args(&job(&e, &v, &a, &outs)).join(" ");
        assert!(s.contains("-fflags +genpts+discardcorrupt -reinit_filter 0 -f mpegts -i pipe:0"));
        assert!(!s.contains("-re "));
        assert!(s.contains("-i tcp://127.0.0.1:40000"));
        assert!(s.contains("overlay=x=0:y=0"));
        assert!(s.contains("-f f32le -ar 48000 -ac 2 -i tcp://127.0.0.1:12346"));
        assert!(s.contains("amix=inputs=2:duration=first:dropout_transition=0:normalize=0"));
        assert!(s.contains("[vprev]fps=5,scale=480:-2[pv]"));
        assert!(s.contains("-c:v libx264 -preset veryfast -tune zerolatency -b:v 3000k -maxrate 3000k -bufsize 6000k -g 50"));
        assert!(s.contains("-f tee [f=flv:onfail=ignore:use_fifo=1:") && s.contains("]rtmp://x/live/key"));
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
        assert!(s.contains("[vin]format=yuv420p[vmix]"));
        assert!(s.contains("[0:v]fps=25:start_time=0[vin]"));
        assert!(s.contains("[0:a]aresample=48000:async=1000:first_pts=0"));
        assert!(s.contains("-f tee [f=null]-"));
        assert_eq!(double_rate("2.5M"), "5M");
    }

    #[test]
    fn deep_settings() {
        let (v, a) = (VideoFormat::default(), AudioFormat::default());
        let e = EncoderConfig {
            rate_control: RateControl::Quality,
            crf: 20,
            maxrate: Some("6000k".into()),
            profile: "high".into(),
            level: "4.1".into(),
            tune: String::new(),
            bframes: Some(2),
            scene_cut: true,
            threads: 4,
            ..Default::default()
        };
        let s = encoder_args(&job(&e, &v, &a, &[])).join(" ");
        assert!(s.contains("-c:v libx264 -preset veryfast -profile:v high -level 4.1 -crf 20 -maxrate 6000k -bufsize 12000k -g 50 -bf 2 -threads 4 -pix_fmt yuv420p"), "{s}");
        assert!(!s.contains("-tune"));
        assert!(!s.contains("-sc_threshold"));

        let e = EncoderConfig { video_codec: "h264_nvenc".into(), rate_control: RateControl::Vbr, ..Default::default() };
        let s = encoder_args(&job(&e, &v, &a, &[])).join(" ");
        assert!(
            s.contains("-c:v h264_nvenc -preset p4 -rc vbr -b:v 3000k -maxrate 4500k -bufsize 9000k -g 50 -keyint_min 50 -no-scenecut 1"),
            "{s}"
        );
    }
}

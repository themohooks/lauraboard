//! Real FFmpeg regression tests; run explicitly with `cargo test -p lb-media
//! --test av_sync -- --ignored`. Requires ffmpeg and ffprobe on PATH.
use std::io::Write;
use std::process::{Command, Stdio};

use lb_core::config::{AudioFormat, EncoderConfig, OutputTarget, VideoFormat};
use lb_media::args::encoder::{EncodeJob, encoder_args};

fn run(program: &str, args: &[String], input: Vec<u8>) -> Vec<u8> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("install FFmpeg to run the A/V regression tests");
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    writer.join().unwrap().unwrap();
    result.stdout
}

fn ffmpeg(args: &[&str], input: Vec<u8>) -> Vec<u8> {
    let mut all: Vec<String> = ["-hide_banner", "-loglevel", "error"].into_iter().map(String::from).collect();
    all.extend(args.iter().map(|s| s.to_string()));
    run("ffmpeg", &all, input)
}

#[test]
#[ignore = "requires FFmpeg"]
fn repeated_item_boundaries_do_not_accumulate_av_drift() {
    // A flash and a tone start together in each item. The video ends one
    // frame before the audio; TS concatenation can also lose a trailing
    // MPEG-2 frame. Independent frame/sample counters accumulated seconds
    // of drift here, even with sample-exact intermediate PCM audio.
    for channels in [2, 1] {
        let (codec, strict) = if channels == 2 { ("s302m", "-2") } else { ("mp2", "0") };
        let clip = ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "color=black:s=160x90:r=25:d=0.48,drawbox=color=white:t=fill:enable='gte(t,0.24)'",
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=if(gte(t\\,0.24)\\,0.3*sin(2*PI*1000*t)\\,0):s=48000:d=0.52",
                "-ac",
                &channels.to_string(),
                "-c:v",
                "mpeg2video",
                "-bf",
                "0",
                "-c:a",
                codec,
                "-strict",
                strict,
                "-f",
                "mpegts",
                "-muxdelay",
                "0",
                "pipe:1",
            ],
            vec![],
        );
        assert_sync(encode(clip.repeat(100), channels), channels, 100);
    }
}

fn encode(input: Vec<u8>, channels: u32) -> Vec<u8> {
    let video = VideoFormat { width: 160, height: 90, ..Default::default() };
    let audio = AudioFormat { channels, ..Default::default() };
    let encoder = EncoderConfig::default();
    let outputs = [OutputTarget { name: "test".into(), url: "pipe:1".into(), enabled: true, format: Some("matroska".into()) }];
    let args = encoder_args(&EncodeJob {
        channel: "test",
        video: &video,
        audio: &audio,
        encoder: &encoder,
        video_codec: "libx264",
        outputs: &outputs,
        record_feed: None,
        gfx: None,
        preview: None,
        loglevel: "error",
    });
    if let Some(dir) = std::env::var_os("LB_AV_SYNC_ARTIFACTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = format!("{}-{channels}", std::thread::current().name().unwrap_or("test"));
        std::fs::write(dir.join(format!("{name}-input.ts")), &input).unwrap();
        std::fs::write(dir.join(format!("{name}-args.json")), serde_json::to_vec_pretty(&args).unwrap()).unwrap();
        let output = run("ffmpeg", &args, input);
        std::fs::write(dir.join(format!("{name}-output.mkv")), &output).unwrap();
        output
    } else {
        run("ffmpeg", &args, input)
    }
}

fn assert_sync(encoded: Vec<u8>, channels: u32, count: usize) {
    let frames = ffmpeg(&["-i", "pipe:0", "-an", "-pix_fmt", "gray", "-f", "rawvideo", "pipe:1"], encoded.clone());
    let samples = ffmpeg(&["-i", "pipe:0", "-vn", "-ac", "1", "-f", "f32le", "pipe:1"], encoded.clone());
    let mut flashes = Vec::new();
    let mut was_white = false;
    for (i, frame) in frames.chunks_exact(160 * 90).enumerate() {
        let white = frame[80] > 128;
        if white && !was_white {
            flashes.push(i as f64 / 25.0);
        }
        was_white = white;
    }
    let tones = tone_times(&samples);
    assert_eq!(flashes.len(), count, "lost flashes with {channels} channels");
    assert_eq!(tones.len(), count, "lost tones with {channels} channels");
    for (i, (flash, tone)) in flashes.iter().zip(&tones).enumerate() {
        assert!((flash - tone).abs() < 0.1, "item {i}, {channels} channels: flash {flash:.3}s, tone {tone:.3}s");
    }
    let probe_args = ["-v", "error", "-show_packets", "-show_entries", "packet=codec_type,pts_time,duration_time", "-of", "json", "pipe:0"];
    let probe = run("ffprobe", &probe_args.map(String::from), encoded);
    let packets: serde_json::Value = serde_json::from_slice(&probe).unwrap();
    let end = |kind: &str| {
        packets["packets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["codec_type"] == kind)
            .map(|p| {
                p["pts_time"].as_str().unwrap().parse::<f64>().unwrap() + p["duration_time"].as_str().unwrap_or("0").parse::<f64>().unwrap()
            })
            .fold(0.0_f64, f64::max)
    };
    assert!((end("video") - end("audio")).abs() < 0.15, "A/V tails diverged");
}

fn tone_times(samples: &[u8]) -> Vec<f64> {
    let mut tones = Vec::new();
    let mut last_loud = None;
    for (i, bytes) in samples.chunks_exact(4).enumerate() {
        if f32::from_le_bytes(bytes.try_into().unwrap()).abs() > 0.1 {
            if last_loud.is_none_or(|last| i - last > 4800) {
                tones.push(i as f64 / 48000.0);
            }
            last_loud = Some(i);
        }
    }
    tones
}

#[test]
#[ignore = "requires FFmpeg"]
fn continuous_audio_clock_drift_is_compensated() {
    // Two minutes with audio clock running 0.5% differently (600 ms of
    // drift without resampling). Tone timestamps still match the flashes.
    let clip = ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=black:s=160x90:r=25:d=120,drawbox=color=white:t=fill:enable='between(mod(t,1),0.24,0.48)'",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=if(between(mod(t*1.005\\,1)\\,0.24\\,0.48)\\,0.3*sin(2*PI*1000*t)\\,0):s=48000:d=119.402985,asetpts=PTS*1.005",
            "-ac",
            "2",
            "-c:v",
            "mpeg2video",
            "-bf",
            "0",
            "-c:a",
            "s302m",
            "-strict",
            "-2",
            "-f",
            "mpegts",
            "-muxdelay",
            "0",
            "pipe:1",
        ],
        vec![],
    );
    assert_sync(encode(clip, 2), 2, 120);
}

#[test]
#[ignore = "requires FFmpeg"]
fn source_audio_clock_drift_is_compensated() {
    // Also exercise the per-item audio chain: equal nominal sample rates
    // must not let FFmpeg 6.1 reset the resampler when drift first appears.
    for normalize in [false, true] {
        let audio = AudioFormat { normalize, ..Default::default() };
        let chain = lb_media::args::filters::audio_chain(&audio, 0.0, 0, 0, 6000);
        let samples = ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=if(between(mod(t*1.005\\,1)\\,0.24\\,0.48)\\,0.3*sin(2*PI*1000*t)\\,0):s=48000:d=5.970149,asetpts=PTS*1.005",
                "-af",
                &chain,
                "-t",
                "6",
                "-ac",
                "1",
                "-f",
                "f32le",
                "pipe:1",
            ],
            vec![],
        );
        let tones = tone_times(&samples);
        assert_eq!(tones.len(), 6, "lost source tones: normalize={normalize}");
        for (i, tone) in tones.iter().enumerate() {
            assert!((tone - (i as f64 + 0.24)).abs() < 0.03, "source tone {i} at {tone:.3}s: normalize={normalize}");
        }
    }
}

#[test]
#[ignore = "requires FFmpeg"]
fn source_audio_delay_is_preserved_as_silence() {
    // A track starting later than the picture must retain that offset,
    // even though the downstream encoder receives continuous PCM samples.
    for normalize in [false, true] {
        let audio = AudioFormat { normalize, ..Default::default() };
        let chain = lb_media::args::filters::audio_chain(&audio, 0.0, 0, 0, 750);
        let samples = ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:sample_rate=48000:duration=0.25",
                "-af",
                &format!("asetpts=PTS+0.25/TB,{chain}"),
                "-t",
                "0.75",
                "-ac",
                "1",
                "-f",
                "f32le",
                "pipe:1",
            ],
            vec![],
        );
        assert_eq!(samples.len() / 4, 36_000);
        let onset = samples.chunks_exact(4).position(|b| f32::from_le_bytes(b.try_into().unwrap()).abs() > 0.01).unwrap();
        assert!((onset as f64 / 48000.0 - 0.25).abs() < 0.01, "audio offset lost: normalize={normalize}");
    }
}

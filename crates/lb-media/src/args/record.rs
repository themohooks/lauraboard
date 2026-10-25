//! Recorder: a separate ffmpeg listening on a local port for the copy of
//! the broadcast the encoder sends it (see [`super::outputs::record_feed_slave`]),
//! writing files cut by time, in the configured container, as is or
//! encoded again.

use lb_core::config::{RecordAudioCodec, RecordConfig, RecordFormat, RecordVideoCodec};

use super::{base_args, sv};

#[derive(Debug, Clone)]
pub struct RecordJob<'a> {
    pub rec: &'a RecordConfig,
    /// Directory (resolved).
    pub dir: &'a str,
    pub channel: &'a str,
    pub port: u16,
    /// Expected bits/s of the recording (for the file size limit).
    pub bitrate: Option<f64>,
    pub loglevel: &'a str,
}

/// Recorder command line. With recording off the feed is still taken (and
/// thrown away), so switching recording on needs no encoder restart.
pub fn recorder_args(j: &RecordJob) -> Vec<String> {
    let rec = j.rec;
    let mut a = base_args(j.loglevel);
    a.retain(|x| x != "-nostdin"); // "q" on stdin closes the files cleanly
    a.extend(sv(&["-fflags", "+discardcorrupt", "-f", "mpegts", "-i", &format!("tcp://127.0.0.1:{}?listen=1", j.port)]));
    if !rec.enabled {
        a.extend(sv(&["-map", "0", "-c", "copy", "-f", "null", "-"]));
        return a;
    }
    match rec.video_codec() {
        None => a.extend(sv(&["-map", "0:a:0", "-vn"])),
        Some(v) => {
            a.extend(sv(&["-map", "0:v:0", "-map", "0:a:0?"]));
            video_args(rec, v, &mut a);
        }
    }
    audio_args(rec, &mut a);
    let seg = rec.segment_secs(j.bitrate);
    let pattern = format!("{}/{}-%Y%m%d-%H%M%S.{}", j.dir.trim_end_matches(['/', '\\']), rec.file_prefix(j.channel), rec.format.extension());
    a.extend(sv(&["-f", "segment", "-segment_time", &seg.to_string(), "-segment_format", muxer(rec.format)]));
    if let Some(o) = muxer_options(rec.format) {
        a.extend(sv(&["-segment_format_options", o]));
    }
    a.extend(sv(&["-strftime", "1", "-reset_timestamps", "1", &pattern]));
    a
}

fn muxer(f: RecordFormat) -> &'static str {
    match f {
        RecordFormat::Mp4 => "mp4",
        RecordFormat::Mkv => "matroska",
        RecordFormat::Ts => "mpegts",
        RecordFormat::Mov => "mov",
        RecordFormat::Flv => "flv",
        RecordFormat::Mp3 => "mp3",
        RecordFormat::M4a => "ipod",
        RecordFormat::Ogg => "ogg",
        RecordFormat::Flac => "flac",
        RecordFormat::Wav => "wav",
    }
}

/// Fragmented MP4 / MOV / M4A: a file cut short by a crash or power loss
/// still plays. Written out packet by packet: the muxer otherwise holds
/// the fragments back for a long time.
fn muxer_options(f: RecordFormat) -> Option<&'static str> {
    match f {
        RecordFormat::Mp4 | RecordFormat::Mov => Some("flush_packets=1:movflags=+frag_keyframe+empty_moov+default_base_moof"),
        // every sound packet is a keyframe: fragments by time instead
        RecordFormat::M4a => Some("flush_packets=1:movflags=+empty_moov+default_base_moof:frag_duration=2000000"),
        _ => None,
    }
}

fn video_args(rec: &RecordConfig, codec: RecordVideoCodec, a: &mut Vec<String>) {
    let v = &rec.video;
    let lib = match codec {
        RecordVideoCodec::Copy => {
            a.extend(sv(&["-c:v", "copy"]));
            return;
        }
        RecordVideoCodec::H264 => "libx264",
        RecordVideoCodec::H265 => "libx265",
    };
    if v.height > 0 {
        a.extend(sv(&["-vf", &format!("scale=-2:{}", v.height)]));
    }
    let preset = if v.preset.trim().is_empty() { "veryfast" } else { v.preset.trim() };
    a.extend(sv(&["-c:v", lib, "-preset", preset]));
    match v.bitrate.trim() {
        "" => a.extend(sv(&["-crf", &v.crf.min(51).to_string()])),
        b => a.extend(sv(&["-b:v", b, "-maxrate", b, "-bufsize", &double(b)])),
    }
    // files are cut at keyframes: one every 2 s
    a.extend(sv(&["-pix_fmt", "yuv420p", "-force_key_frames", "expr:gte(t,n_forced*2)"]));
    if codec == RecordVideoCodec::H265 && matches!(rec.format, RecordFormat::Mp4 | RecordFormat::Mov) {
        a.extend(sv(&["-tag:v", "hvc1"]));
    }
}

fn audio_args(rec: &RecordConfig, a: &mut Vec<String>) {
    let s = &rec.audio;
    let codec = rec.audio_codec();
    let hi = s.bits >= 24;
    match codec {
        RecordAudioCodec::Copy => {
            a.extend(sv(&["-c:a", "copy"]));
            return;
        }
        RecordAudioCodec::Aac => a.extend(sv(&["-c:a", "aac"])),
        RecordAudioCodec::Mp3 => a.extend(sv(&["-c:a", "libmp3lame"])),
        RecordAudioCodec::Opus => a.extend(sv(&["-c:a", "libopus"])),
        RecordAudioCodec::Flac => a.extend(sv(&["-c:a", "flac", "-sample_fmt", if hi { "s32" } else { "s16" }])),
        RecordAudioCodec::Pcm => a.extend(sv(&["-c:a", if hi { "pcm_s24le" } else { "pcm_s16le" }])),
    }
    if codec.has_bitrate() {
        let b = if s.bitrate.trim().is_empty() { "192k" } else { s.bitrate.trim() };
        a.extend(sv(&["-b:a", b]));
    }
    if codec == RecordAudioCodec::Flac && hi {
        a.extend(sv(&["-bits_per_raw_sample:a", "24"]));
    }
    // Opus runs at 48 kHz only (or lower telephone rates)
    let rate = match (codec, s.sample_rate) {
        (RecordAudioCodec::Opus, r) if ![8000, 12000, 16000, 24000, 48000].contains(&r) => 48000,
        (_, r) => r,
    };
    if rate > 0 {
        a.extend(sv(&["-ar", &rate.to_string()]));
    }
    if s.channels > 0 {
        a.extend(sv(&["-ac", &s.channels.to_string()]));
    }
}

/// "3000k" → "6000k".
fn double(r: &str) -> String {
    let (num, unit) = r.split_at(r.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(r.len()));
    match num.parse::<f64>() {
        Ok(n) => format!("{}{unit}", (n * 2.0).round() as i64),
        Err(_) => r.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::Dur;

    fn job(rec: &RecordConfig) -> Vec<String> {
        recorder_args(&RecordJob { rec, dir: "D:\\tv\\rec\\", channel: "main", port: 41000, bitrate: Some(4_000_000.0), loglevel: "warning" })
    }

    fn has(a: &[String], pair: &[&str]) -> bool {
        a.windows(pair.len()).any(|w| w.iter().zip(pair).all(|(x, y)| x == y))
    }

    #[test]
    fn off_takes_the_feed() {
        let a = job(&RecordConfig::default());
        assert!(has(&a, &["-i", "tcp://127.0.0.1:41000?listen=1"]));
        assert!(has(&a, &["-f", "null", "-"]));
    }

    #[test]
    fn copy_as_on_air() {
        let rec = RecordConfig { enabled: true, segment: Dur::mins(30), format: RecordFormat::Ts, ..Default::default() };
        let a = job(&rec);
        assert!(has(&a, &["-c:v", "copy"]) && has(&a, &["-c:a", "copy"]));
        assert!(has(&a, &["-segment_time", "1800", "-segment_format", "mpegts"]));
        assert_eq!(a.last().unwrap(), "D:\\tv\\rec/main-%Y%m%d-%H%M%S.ts");
    }

    #[test]
    fn encoded_again() {
        let mut rec = RecordConfig { enabled: true, format: RecordFormat::Mkv, max_file_mb: 500, ..Default::default() };
        rec.video.codec = RecordVideoCodec::H265;
        rec.video.height = 720;
        rec.video.bitrate = "2500k".into();
        rec.audio.codec = RecordAudioCodec::Flac;
        rec.audio.bits = 24;
        rec.audio.sample_rate = 96000;
        let a = job(&rec);
        assert!(has(&a, &["-vf", "scale=-2:720", "-c:v", "libx265"]), "{a:?}");
        assert!(has(&a, &["-b:v", "2500k", "-maxrate", "2500k", "-bufsize", "5000k"]));
        assert!(has(&a, &["-c:a", "flac", "-sample_fmt", "s32"]) && has(&a, &["-ar", "96000"]));
        assert!(has(&a, &["-segment_time", "970", "-segment_format", "matroska"]));
    }

    #[test]
    fn sound_only() {
        let mut rec = RecordConfig { enabled: true, format: RecordFormat::Ogg, ..Default::default() };
        rec.audio.sample_rate = 44100;
        rec.audio.channels = 1;
        rec.audio.bitrate = "96k".into();
        let a = job(&rec);
        assert!(has(&a, &["-map", "0:a:0", "-vn", "-c:a", "libopus", "-b:a", "96k", "-ar", "48000", "-ac", "1"]), "{a:?}");
        assert!(!a.iter().any(|x| x == "-c:v"));
        let m4a = job(&RecordConfig { enabled: true, format: RecordFormat::M4a, ..Default::default() });
        assert!(has(&m4a, &["-c:a", "aac", "-b:a", "192k"]) && has(&m4a, &["-segment_format", "ipod"]));
        assert!(m4a.last().unwrap().ends_with(".m4a"));
    }
}

//! ffprobe wrapper: duration and stream info.

use serde::Deserialize;

use lb_core::media::{ProbeInfo, VideoInfo};

use crate::error::{MediaError, Result};
use crate::process::command;

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeResult {
    pub duration_ms: i64,
    pub info: ProbeInfo,
}

#[derive(Deserialize)]
struct Out {
    #[serde(default)]
    streams: Vec<Stream>,
    #[serde(default)]
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
    format_name: Option<String>,
}

fn rate(s: &Option<String>) -> Option<f64> {
    let s = s.as_deref()?;
    let (n, d) = s.split_once('/').unwrap_or((s, "1"));
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d > 0.0 && n > 0.0).then_some(n / d)
}

/// Parse ffprobe `-of json -show_format -show_streams` output.
pub fn parse_probe_json(json: &str) -> std::result::Result<ProbeResult, String> {
    let out: Out = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let video = out.streams.iter().find(|s| s.codec_type.as_deref() == Some("video"));
    let audio = out.streams.iter().find(|s| s.codec_type.as_deref() == Some("audio"));
    let secs = out
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .or_else(|| video.and_then(|v| v.duration.as_deref()))
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);
    let info = ProbeInfo {
        video: video.map(|v| VideoInfo {
            width: v.width.unwrap_or(0),
            height: v.height.unwrap_or(0),
            fps: rate(&v.avg_frame_rate).or_else(|| rate(&v.r_frame_rate)).unwrap_or(0.0),
            codec: v.codec_name.clone().unwrap_or_default(),
        }),
        has_audio: audio.is_some(),
        audio_codec: audio.and_then(|a| a.codec_name.clone()).unwrap_or_default(),
        format: out.format.and_then(|f| f.format_name).unwrap_or_default(),
        error: None,
    };
    if info.video.is_none() && !info.has_audio {
        return Err("no audio or video streams".into());
    }
    Ok(ProbeResult { duration_ms: (secs * 1000.0).round() as i64, info })
}

/// Run ffprobe on a file or URL.
pub async fn probe(ffprobe: &str, input: &str) -> Result<ProbeResult> {
    let out = command(ffprobe)
        .args(["-v", "error", "-of", "json", "-show_format", "-show_streams", input])
        .output()
        .await
        .map_err(|e| MediaError::Spawn { program: ffprobe.to_string(), source: e })?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(MediaError::Probe { input: input.into(), message: if msg.is_empty() { "unknown error".into() } else { msg } });
    }
    parse_probe_json(&String::from_utf8_lossy(&out.stdout)).map_err(|m| MediaError::Probe { input: input.into(), message: m })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_output() {
        let json = r#"{"streams":[
            {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"30000/1001"},
            {"codec_type":"audio","codec_name":"aac"}],
            "format":{"duration":"12.345","format_name":"mov,mp4"}}"#;
        let r = parse_probe_json(json).unwrap();
        assert_eq!(r.duration_ms, 12_345);
        let v = r.info.video.unwrap();
        assert_eq!((v.width, v.height), (1920, 1080));
        assert!((v.fps - 29.97).abs() < 0.01);
        assert!(r.info.has_audio);
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_probe_json(r#"{"streams":[],"format":{}}"#).is_err());
    }
}

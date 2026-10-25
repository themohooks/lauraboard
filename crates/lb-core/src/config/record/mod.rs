//! Recording of the broadcast to local files (`[record]` in `channel.toml`).

use serde::{Deserialize, Serialize};

use super::{AudioFormat, EncoderConfig, RateControl};
use crate::Dur;

mod codec;
pub use codec::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Directory for recordings: absolute, or relative to the channel dir.
    #[serde(default = "RecordConfig::dir")]
    pub dir: String,
    #[serde(default)]
    pub format: RecordFormat,
    /// Files are split by this length.
    #[serde(default = "RecordConfig::seg")]
    pub segment: Dur,
    /// Largest file size in megabytes (0 = no limit). Files are cut at
    /// keyframes by time, so the limit is converted to a length using the
    /// encoder's bitrate: files come out close to, and normally under it.
    #[serde(default)]
    pub max_file_mb: u64,
    /// Delete recordings older than this many days (0 = keep).
    #[serde(default)]
    pub keep_days: u32,
    /// Keep the recordings of the channel under this many gigabytes in
    /// total, deleting the oldest files (0 = no limit).
    #[serde(default)]
    pub max_total_gb: f64,
    /// File name prefix; empty = the channel id. Files are named
    /// `<prefix>-YYYYMMDD-HHMMSS.<ext>`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    /// Picture: as on air, or encoded again (other codec, size, quality).
    #[serde(default)]
    pub video: RecordVideo,
    /// Sound: as on air, or encoded again (codec, bitrate, sample rate).
    #[serde(default)]
    pub audio: RecordAudio,
}

impl RecordConfig {
    fn dir() -> String {
        "recordings".into()
    }
    fn seg() -> Dur {
        Dur::mins(60)
    }

    /// File name prefix for `channel`, safe for file names.
    pub fn file_prefix(&self, channel: &str) -> String {
        let p = if self.prefix.trim().is_empty() { channel } else { self.prefix.trim() };
        p.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()
    }

    /// Length of one file in seconds: the configured segment, shortened so
    /// a file at `bitrate` bits/s stays under `max_file_mb`.
    pub fn segment_secs(&self, bitrate: Option<f64>) -> u64 {
        let by_time = (self.segment.ms() / 1000).max(60) as u64;
        match (self.max_file_mb, bitrate) {
            (mb, Some(bps)) if mb > 0 && bps > 0.0 => {
                // 3 % for container overhead and keyframe overshoot
                let secs = (mb as f64 * 8_000_000.0 * 0.97 / bps).floor() as u64;
                by_time.min(secs.max(10))
            }
            _ => by_time,
        }
    }
}

impl RecordConfig {
    /// Picture codec in effect (`None`: sound only). One the container
    /// does not take falls back to copying.
    pub fn video_codec(&self) -> Option<RecordVideoCodec> {
        let ok = self.format.video_codecs();
        if ok.is_empty() {
            return None;
        }
        Some(if ok.contains(&self.video.codec) { self.video.codec } else { ok[0] })
    }

    /// Sound codec in effect: the configured one, or the container's first.
    pub fn audio_codec(&self) -> RecordAudioCodec {
        let ok = self.format.audio_codecs();
        if ok.contains(&self.audio.codec) { self.audio.codec } else { ok[0] }
    }

    /// Expected bitrate of the recording (bits/s), for the file size limit.
    pub fn expected_bitrate(&self, e: &EncoderConfig, a: &AudioFormat) -> Option<f64> {
        let video = match self.video_codec() {
            None => 0.0,
            Some(RecordVideoCodec::Copy) => video_bitrate(e)?,
            Some(_) => parse_rate(&self.video.bitrate).or_else(|| video_bitrate(e))?,
        };
        let sr = if self.audio.sample_rate > 0 { self.audio.sample_rate } else { a.sample_rate } as f64;
        let ch = if self.audio.channels > 0 { self.audio.channels } else { a.channels } as f64;
        let pcm = sr * ch * if self.audio.bits >= 24 { 24.0 } else { 16.0 };
        let audio = match self.audio_codec() {
            RecordAudioCodec::Copy => parse_rate(&e.audio_bitrate).unwrap_or(128_000.0),
            RecordAudioCodec::Pcm => pcm,
            // lossless compression: about 60 % of PCM for broadcast sound
            RecordAudioCodec::Flac => pcm * 0.6,
            _ => parse_rate(&self.audio.bitrate).unwrap_or(192_000.0),
        };
        Some(video + audio)
    }
}

impl Default for RecordConfig {
    fn default() -> Self {
        RecordConfig {
            enabled: false,
            dir: Self::dir(),
            format: RecordFormat::default(),
            segment: Self::seg(),
            max_file_mb: 0,
            keep_days: 0,
            max_total_gb: 0.0,
            prefix: String::new(),
            video: RecordVideo::default(),
            audio: RecordAudio::default(),
        }
    }
}

/// `3000k`, `6M`, `128000` → bits per second.
pub fn parse_rate(s: &str) -> Option<f64> {
    let s = s.trim();
    let (num, k) = match s.chars().last()? {
        'k' | 'K' => (&s[..s.len() - 1], 1e3),
        'm' | 'M' => (&s[..s.len() - 1], 1e6),
        _ => (s, 1.0),
    };
    num.trim().parse::<f64>().ok().map(|n| n * k).filter(|v| *v > 0.0)
}

/// Expected total bitrate of the encoder output (bits/s): the configured
/// video rate (peaks for variable and quality modes) plus audio. For
/// constant quality without a cap the rate is unknown: twice the nominal
/// bitrate is assumed, so files stay under a size limit.
pub fn expected_bitrate(e: &EncoderConfig) -> Option<f64> {
    Some(video_bitrate(e)? + parse_rate(&e.audio_bitrate).unwrap_or(128_000.0))
}

/// Expected video bitrate of the encoder output (bits/s).
fn video_bitrate(e: &EncoderConfig) -> Option<f64> {
    let nominal = parse_rate(&e.video_bitrate);
    let cap = e.maxrate.as_deref().and_then(parse_rate);
    match e.rate_control {
        RateControl::Cbr => nominal,
        RateControl::Vbr => cap.or(nominal.map(|n| n * 1.5)),
        RateControl::Quality => cap.or(nominal.map(|n| n * 2.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates() {
        assert_eq!(parse_rate("3000k"), Some(3_000_000.0));
        assert_eq!(parse_rate("6M"), Some(6_000_000.0));
        assert_eq!(parse_rate("x"), None);
        let e = EncoderConfig { video_bitrate: "3000k".into(), audio_bitrate: "128k".into(), ..Default::default() };
        assert_eq!(expected_bitrate(&e), Some(3_128_000.0));
    }

    #[test]
    fn size_limit_shortens_files() {
        let r = RecordConfig { max_file_mb: 1000, ..Default::default() };
        // 1000 MB at ~4 Mbit/s ≈ 32 min, under the 60 min segment
        let s = r.segment_secs(Some(4_000_000.0));
        assert!((1900..=1950).contains(&s), "{s}");
        assert_eq!(RecordConfig::default().segment_secs(Some(4_000_000.0)), 3600);
        assert_eq!(r.segment_secs(None), 3600);
    }

    #[test]
    fn old_config_parses() {
        let r: RecordConfig = toml::from_str("enabled = true\ndir = \"rec\"\nsegment = \"00:30:00\"").unwrap();
        assert_eq!(r.format, RecordFormat::Mp4);
        assert_eq!(r.segment_secs(None), 1800);
        assert_eq!(RecordConfig { prefix: "Мой канал/1".into(), ..r.clone() }.file_prefix("main"), "Мой_канал_1");
        assert_eq!(r.video_codec(), Some(RecordVideoCodec::Copy));
        assert_eq!(r.audio_codec(), RecordAudioCodec::Copy);
    }

    #[test]
    fn codecs_follow_the_container() {
        let mut r = RecordConfig { format: RecordFormat::Flv, ..Default::default() };
        r.video.codec = RecordVideoCodec::H265;
        r.audio.codec = RecordAudioCodec::Flac;
        // FLV takes neither: copy
        assert_eq!(r.video_codec(), Some(RecordVideoCodec::Copy));
        assert_eq!(r.audio_codec(), RecordAudioCodec::Copy);
        r.format = RecordFormat::Wav;
        assert_eq!(r.video_codec(), None);
        assert_eq!(r.audio_codec(), RecordAudioCodec::Pcm);
    }

    #[test]
    fn audio_only_bitrate() {
        let (e, a) = (EncoderConfig::default(), AudioFormat::default());
        let mut r = RecordConfig { format: RecordFormat::Mp3, ..Default::default() };
        r.audio.bitrate = "320k".into();
        assert_eq!(r.expected_bitrate(&e, &a), Some(320_000.0));
        r.format = RecordFormat::Wav;
        r.audio.bits = 24;
        let pcm = a.sample_rate as f64 * a.channels as f64 * 24.0;
        assert_eq!(r.expected_bitrate(&e, &a), Some(pcm));
        let video: RecordConfig = Default::default();
        assert_eq!(video.expected_bitrate(&e, &a), expected_bitrate(&e));
    }
}

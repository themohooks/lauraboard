//! Per-channel settings (`channels/<id>/channel.toml`).

use serde::{Deserialize, Serialize};

use crate::Dur;
use crate::planner::PlanMode;

use super::RecordConfig;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    /// Keep aspect ratio, pad with background (letter/pillar box).
    #[default]
    Fit,
    /// Keep aspect ratio, crop to fill.
    Fill,
    /// Ignore aspect ratio.
    Stretch,
    /// Keep aspect ratio; the bars show a blurred, zoomed copy of the
    /// picture instead of a flat colour.
    Blur,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoFormat {
    #[serde(default = "VideoFormat::w")]
    pub width: u32,
    #[serde(default = "VideoFormat::h")]
    pub height: u32,
    #[serde(default = "VideoFormat::fps")]
    pub fps: f64,
    #[serde(default)]
    pub scale: ScaleMode,
    /// Padding / slate colour.
    #[serde(default = "VideoFormat::black")]
    pub background: String,
}

impl VideoFormat {
    fn w() -> u32 {
        1280
    }
    fn h() -> u32 {
        720
    }
    fn fps() -> f64 {
        25.0
    }
    fn black() -> String {
        "#000000".into()
    }
    pub fn aspect(&self) -> f64 {
        self.width as f64 / self.height.max(1) as f64
    }
}

impl Default for VideoFormat {
    fn default() -> Self {
        VideoFormat { width: Self::w(), height: Self::h(), fps: Self::fps(), scale: ScaleMode::Fit, background: Self::black() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioFormat {
    #[serde(default = "AudioFormat::sr")]
    pub sample_rate: u32,
    #[serde(default = "AudioFormat::ch")]
    pub channels: u32,
    /// Master gain in dB.
    #[serde(default)]
    pub volume_db: f32,
    /// EBU R128 loudness normalisation per item (`loudnorm`).
    #[serde(default)]
    pub normalize: bool,
}

impl AudioFormat {
    fn sr() -> u32 {
        48_000
    }
    fn ch() -> u32 {
        2
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        AudioFormat { sample_rate: Self::sr(), channels: Self::ch(), volume_db: 0.0, normalize: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncoderConfig {
    /// `libx264`, `h264_nvenc`, `h264_qsv`, `h264_amf`, `h264_vaapi` or `auto`.
    #[serde(default = "EncoderConfig::vcodec")]
    pub video_codec: String,
    #[serde(default = "EncoderConfig::preset")]
    pub preset: String,
    #[serde(default = "EncoderConfig::vbr")]
    pub video_bitrate: String,
    #[serde(default)]
    pub maxrate: Option<String>,
    #[serde(default)]
    pub bufsize: Option<String>,
    /// Keyframe interval in seconds.
    #[serde(default = "EncoderConfig::gop")]
    pub gop_seconds: f64,
    #[serde(default = "EncoderConfig::acodec")]
    pub audio_codec: String,
    #[serde(default = "EncoderConfig::abr")]
    pub audio_bitrate: String,
    /// Extra raw ffmpeg output arguments.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Constant bitrate (default), variable bitrate, or constant quality.
    #[serde(default)]
    pub rate_control: RateControl,
    /// Quality for `rate_control = "quality"` (x264 CRF scale, lower = better).
    #[serde(default = "EncoderConfig::crf")]
    pub crf: u32,
    /// H.264 / HEVC profile (`baseline`, `main`, `high`); empty = encoder default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile: String,
    /// Level (`4.1`, `4.2`, …); empty = automatic.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub level: String,
    /// x264 / x265 tune; empty = none.
    #[serde(default = "EncoderConfig::tune")]
    pub tune: String,
    /// B-frames between reference frames; `None` = encoder default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bframes: Option<u32>,
    /// Extra keyframes on scene cuts (off: strictly regular GOP, which
    /// HLS segmenting and most platforms want).
    #[serde(default)]
    pub scene_cut: bool,
    /// Encoder threads; 0 = automatic.
    #[serde(default)]
    pub threads: u32,
    /// Output pixel format (`yuv420p` for platforms and players).
    #[serde(default = "EncoderConfig::pix_fmt")]
    pub pix_fmt: String,
}

/// How the encoder spends bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RateControl {
    /// Steady bitrate: what streaming platforms expect.
    #[default]
    Cbr,
    /// Average bitrate with peaks up to `maxrate`.
    Vbr,
    /// Constant quality (`crf`), capped by `maxrate`.
    Quality,
}

impl EncoderConfig {
    fn vcodec() -> String {
        "libx264".into()
    }
    fn preset() -> String {
        "veryfast".into()
    }
    fn vbr() -> String {
        "3000k".into()
    }
    fn gop() -> f64 {
        2.0
    }
    fn acodec() -> String {
        "aac".into()
    }
    fn abr() -> String {
        "160k".into()
    }
    fn crf() -> u32 {
        23
    }
    fn tune() -> String {
        "zerolatency".into()
    }
    fn pix_fmt() -> String {
        "yuv420p".into()
    }
}

impl Default for EncoderConfig {
    fn default() -> Self {
        EncoderConfig {
            video_codec: Self::vcodec(),
            preset: Self::preset(),
            video_bitrate: Self::vbr(),
            maxrate: None,
            bufsize: None,
            gop_seconds: Self::gop(),
            audio_codec: Self::acodec(),
            audio_bitrate: Self::abr(),
            extra_args: vec![],
            rate_control: RateControl::default(),
            crf: Self::crf(),
            profile: String::new(),
            level: String::new(),
            tune: Self::tune(),
            bframes: None,
            scene_cut: false,
            threads: 0,
            pix_fmt: Self::pix_fmt(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputTarget {
    pub name: String,
    /// `rtmp://…/app/key`, `srt://…`, `udp://…`, a file path, `…/index.m3u8`.
    pub url: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Container format override (`flv`, `mpegts`, `hls`); guessed from url.
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "PreviewConfig::w")]
    pub width: u32,
    /// Frames per second; see [`PreviewConfig::effective_fps`].
    #[serde(default = "PreviewConfig::fps")]
    pub fps: f64,
}

impl PreviewConfig {
    /// Fewer frames than this looks like a slideshow to the operator.
    pub const MIN_FPS: f64 = 10.0;

    fn w() -> u32 {
        640
    }
    fn fps() -> f64 {
        15.0
    }

    /// Rate actually produced: at least [`Self::MIN_FPS`] (older channels
    /// stored 5), never above the video rate.
    pub fn effective_fps(&self, video_fps: f64) -> f64 {
        self.fps.max(Self::MIN_FPS).min(video_fps.max(1.0))
    }
}

impl Default for PreviewConfig {
    fn default() -> Self {
        PreviewConfig { enabled: true, width: Self::w(), fps: Self::fps() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FallbackConfig {
    /// Standby image used for slates and when nothing can play.
    #[serde(default)]
    pub image: Option<String>,
    /// Backup media/stream played when the rundown is empty or failing.
    #[serde(default)]
    pub media: Option<String>,
    /// Text drawn on the generated slate.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanningConfig {
    #[serde(flatten)]
    pub mode: PlanMode,
    /// How far ahead the rundown is computed.
    #[serde(default = "PlanningConfig::horizon")]
    pub horizon: Dur,
    #[serde(default)]
    pub seed: u64,
    /// Keep history (for exclusion & ad caps) this long.
    #[serde(default = "PlanningConfig::history")]
    pub history: Dur,
}

impl PlanningConfig {
    fn horizon() -> Dur {
        Dur::mins(24 * 60)
    }
    fn history() -> Dur {
        Dur::mins(14 * 24 * 60)
    }
}

impl Default for PlanningConfig {
    fn default() -> Self {
        PlanningConfig { mode: PlanMode::Grid, horizon: Self::horizon(), seed: 0, history: Self::history() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphicsOutput {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Legacy and ignored: the overlay is rendered at the video frame rate
    /// (a slower overlay makes tickers and slides stutter).
    #[serde(default = "GraphicsOutput::fps")]
    pub fps: f64,
    /// Active design profile.
    #[serde(default)]
    pub profile: Option<String>,
}

impl GraphicsOutput {
    fn fps() -> f64 {
        10.0
    }
}

impl Default for GraphicsOutput {
    fn default() -> Self {
        GraphicsOutput { enabled: true, fps: Self::fps(), profile: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalHlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "hls_port")]
    pub port: u16,
}
fn hls_port() -> u16 {
    8788
}
impl Default for LocalHlsConfig {
    fn default() -> Self {
        Self { enabled: false, port: hls_port() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelConfig {
    #[serde(default)]
    pub local_hls: LocalHlsConfig,
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Start broadcasting automatically when the daemon starts.
    #[serde(default = "yes")]
    pub autostart: bool,
    #[serde(default)]
    pub video: VideoFormat,
    #[serde(default)]
    pub audio: AudioFormat,
    #[serde(default)]
    pub encoder: EncoderConfig,
    #[serde(default)]
    pub outputs: Vec<OutputTarget>,
    #[serde(default)]
    pub record: RecordConfig,
    #[serde(default)]
    pub preview: PreviewConfig,
    #[serde(default)]
    pub fallback: FallbackConfig,
    #[serde(default)]
    pub planning: PlanningConfig,
    #[serde(default)]
    pub graphics: GraphicsOutput,
}

impl ChannelConfig {
    pub fn new(id: &str) -> Self {
        ChannelConfig {
            local_hls: LocalHlsConfig::default(),
            id: id.into(),
            name: id.into(),
            enabled: true,
            autostart: true,
            video: VideoFormat::default(),
            audio: AudioFormat::default(),
            encoder: EncoderConfig::default(),
            outputs: vec![],
            record: RecordConfig::default(),
            preview: PreviewConfig::default(),
            fallback: FallbackConfig::default(),
            planning: PlanningConfig::default(),
            graphics: GraphicsOutput::default(),
        }
    }
}

//! Containers and codecs of recordings: which codec fits which container,
//! and the picture / sound settings of the recorder.

use serde::{Deserialize, Serialize};

/// Container of the recorded files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordFormat {
    /// Fragmented MP4: plays everywhere, stays readable if the program or
    /// the computer stops mid-file.
    #[default]
    Mp4,
    /// Matroska.
    Mkv,
    /// MPEG transport stream (the most robust, largest files).
    Ts,
    /// QuickTime (fragmented, like MP4): for editing software.
    Mov,
    /// Flash video.
    Flv,
    /// Sound only: MP3.
    Mp3,
    /// Sound only: AAC in MP4 (fragmented).
    M4a,
    /// Sound only: Opus in Ogg.
    Ogg,
    /// Sound only, lossless: FLAC.
    Flac,
    /// Sound only, uncompressed: WAV.
    Wav,
}

impl RecordFormat {
    pub const ALL: [RecordFormat; 10] = [
        RecordFormat::Mp4,
        RecordFormat::Mkv,
        RecordFormat::Ts,
        RecordFormat::Mov,
        RecordFormat::Flv,
        RecordFormat::Mp3,
        RecordFormat::M4a,
        RecordFormat::Ogg,
        RecordFormat::Flac,
        RecordFormat::Wav,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            RecordFormat::Mp4 => "mp4",
            RecordFormat::Mkv => "mkv",
            RecordFormat::Ts => "ts",
            RecordFormat::Mov => "mov",
            RecordFormat::Flv => "flv",
            RecordFormat::Mp3 => "mp3",
            RecordFormat::M4a => "m4a",
            RecordFormat::Ogg => "ogg",
            RecordFormat::Flac => "flac",
            RecordFormat::Wav => "wav",
        }
    }

    /// Every extension the recorder writes.
    pub fn extensions() -> impl Iterator<Item = &'static str> {
        Self::ALL.iter().map(|f| f.extension())
    }

    /// Sound without picture.
    pub fn audio_only(self) -> bool {
        matches!(self, RecordFormat::Mp3 | RecordFormat::M4a | RecordFormat::Ogg | RecordFormat::Flac | RecordFormat::Wav)
    }

    /// Picture codecs the container takes (empty: sound only).
    pub fn video_codecs(self) -> &'static [RecordVideoCodec] {
        use RecordVideoCodec::*;
        match self {
            RecordFormat::Flv => &[Copy, H264],
            f if f.audio_only() => &[],
            _ => &[Copy, H264, H265],
        }
    }

    /// Sound codecs the container takes; the first is the default.
    pub fn audio_codecs(self) -> &'static [RecordAudioCodec] {
        use RecordAudioCodec::*;
        match self {
            RecordFormat::Mp4 | RecordFormat::Ts | RecordFormat::Flv => &[Copy, Aac, Mp3],
            RecordFormat::Mov => &[Copy, Aac, Mp3, Pcm],
            RecordFormat::Mkv => &[Copy, Aac, Mp3, Opus, Flac, Pcm],
            RecordFormat::Mp3 => &[Mp3],
            RecordFormat::M4a => &[Aac],
            RecordFormat::Ogg => &[Opus],
            RecordFormat::Flac => &[Flac],
            RecordFormat::Wav => &[Pcm],
        }
    }
}

/// Picture of the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordVideoCodec {
    /// As it goes on air (no extra work for the computer).
    #[default]
    Copy,
    H264,
    H265,
}

/// Sound of the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordAudioCodec {
    /// As it goes on air.
    #[default]
    Copy,
    Aac,
    Mp3,
    Opus,
    Flac,
    /// Uncompressed PCM.
    Pcm,
}

impl RecordAudioCodec {
    /// Takes a bitrate (lossy codecs).
    pub fn has_bitrate(self) -> bool {
        matches!(self, RecordAudioCodec::Aac | RecordAudioCodec::Mp3 | RecordAudioCodec::Opus)
    }
    /// Takes a sample depth (lossless codecs).
    pub fn has_bits(self) -> bool {
        matches!(self, RecordAudioCodec::Flac | RecordAudioCodec::Pcm)
    }
}

/// Picture settings when the recording is encoded on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordVideo {
    pub codec: RecordVideoCodec,
    /// Bitrate (`4000k`); empty = constant quality `crf`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bitrate: String,
    /// Quality (CRF scale, lower = better) when no bitrate is set.
    pub crf: u32,
    pub preset: String,
    /// Picture height (width follows the aspect); 0 = as on air.
    pub height: u32,
}

impl Default for RecordVideo {
    fn default() -> Self {
        RecordVideo { codec: RecordVideoCodec::Copy, bitrate: String::new(), crf: 23, preset: "veryfast".into(), height: 0 }
    }
}

/// Sound settings of the recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordAudio {
    pub codec: RecordAudioCodec,
    /// Bitrate of lossy codecs (`192k`).
    pub bitrate: String,
    /// Hz; 0 = as on air.
    pub sample_rate: u32,
    /// 1 = mono, 2 = stereo; 0 = as on air.
    pub channels: u32,
    /// Sample depth of FLAC and PCM: 16 or 24.
    pub bits: u32,
}

impl Default for RecordAudio {
    fn default() -> Self {
        RecordAudio { codec: RecordAudioCodec::Copy, bitrate: "192k".into(), sample_rate: 0, channels: 0, bits: 16 }
    }
}

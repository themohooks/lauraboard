//! Media engine built on external FFmpeg processes.
//!
//! The pipeline (one channel):
//!
//! ```text
//!  decoder (per item) ──mpegts──▶ feeder (engine) ──stdin──▶ encoder ──▶ RTMP / SRT / file / HLS
//!                                       graphics RGBA frames ──tcp──▶ ┘└──▶ MJPEG preview (stdout)
//! ```
//!
//! * [`args`] — pure, unit-tested builders for ffmpeg command lines
//! * [`probe`] — ffprobe wrapper
//! * [`process`] — spawning and supervising ffmpeg processes
//! * [`progress`] — `-progress` output parser
//! * [`jpeg`] — splitting an MJPEG byte stream into frames
//! * [`hw`] — hardware encoder detection

pub mod args;
pub mod error;
pub mod hw;
pub mod jpeg;
pub mod probe;
pub mod process;
pub mod progress;
pub mod redact;

pub use error::{MediaError, Result};

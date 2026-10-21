//! Configuration files.
//!
//! * [`app`] — system settings (`laurabrod.toml`): FFmpeg, API, logging
//! * [`channel`] — per-channel settings (`channel.toml`): video/audio format,
//!   encoder, outputs, recording, preview, fallback, planning

mod app;
mod channel;

pub use app::*;
pub use channel::*;

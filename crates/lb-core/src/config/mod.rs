//! Configuration files.
//!
//! * [`app`] — system settings (`laurabrod.toml`): FFmpeg, API, logging
//! * [`channel`] — per-channel settings (`channel.toml`): video/audio format,
//!   encoder, outputs, preview, fallback, planning
//! * [`record`] — recording of the broadcast to files

mod app;
mod channel;
mod record;

pub use app::*;
pub use channel::*;
pub use record::*;

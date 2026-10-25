//! Spawning and supervising ffmpeg processes.
//!
//! * [`decoder`] — per-item decoder producing the intermediate stream
//! * [`encoder`] — long-running encoder with stats and preview
//! * [`recorder`] — recording to files from the encoder's local feed
//! * [`stderr`] — stderr reader: logging, progress parsing, last error

pub mod decoder;
pub mod encoder;
pub mod recorder;
pub mod stderr;

pub use decoder::Decoder;
pub use encoder::Encoder;
pub use recorder::Recorder;

use tokio::process::Command;

/// A `Command` configured for background ffmpeg use: killed when dropped,
/// no console window on Windows.
pub fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    c.kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

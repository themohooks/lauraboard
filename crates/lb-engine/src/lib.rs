//! Playout engine: runs channels 24/7 on top of the planner (`lb-core`),
//! the FFmpeg media engine (`lb-media`) and the graphics compositor
//! (`lb-gfx`). Interfaces (API, CLI, GUI) talk to channels only through
//! [`channel::ChannelHandle`].
//!
//! * [`engine`] — multi-channel registry
//! * [`channel`] — channel actor and its handle
//! * [`persist`] — runtime state, as-run log, instance lock
//! * [`library`] — media import / probing
//! * [`stats`] — reports and CSV export
//! * [`notify`] — critical event notifications

pub mod channel;
pub mod engine;
pub mod error;
pub mod library;
pub mod notify;
pub mod persist;
pub mod stats;

pub use channel::{ChannelHandle, ChannelStatus, Command, Event};
pub use engine::Engine;
pub use error::{EngineError, Result};

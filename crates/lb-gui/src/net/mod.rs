//! Networking off the UI thread.
//!
//! * [`api`] — blocking HTTP client
//! * [`worker`] — runs requests on background threads, delivers [`Msg`]s
//! * [`poller`] — periodic status / rundown / preview refresh
//! * [`events`] — WebSocket event stream

pub mod api;
pub mod events;
pub mod poller;
pub mod worker;

use std::sync::Arc;

use serde_json::Value;

use lb_core::graphics::GraphicsConfig;
use lb_core::media::MediaItem;
use lb_core::playlist::Playlist;
use lb_core::rules::RuleSet;
use lb_core::rundown::Rundown;
use lb_core::schedule::ScheduleSet;

use crate::model::StatusView;

/// Wakes the UI thread after a message was queued (toolkit-agnostic).
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// A decoded preview frame (RGBA8, unpremultiplied).
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Results delivered to the UI thread.
pub enum Msg {
    Channels(Vec<crate::model::ChannelEntry>),
    Status(Box<StatusView>),
    Rundown(Rundown),
    Library(Vec<MediaItem>),
    Playlists(Vec<Playlist>),
    Schedules(ScheduleSet),
    Rules(RuleSet),
    Graphics(GraphicsConfig),
    Preview(Frame),
    /// Media thumbnail (`None`: not available for this item).
    Thumb(String, Option<Frame>),
    Event(Value),
    /// A command / save succeeded (message for the user).
    Done(String),
    Error(String),
    Connection(bool),
    /// Run on the UI thread with the bridge context (results of multi-step jobs).
    Apply(Box<dyn FnOnce(&mut crate::bridge::Ctx) + Send>),
}

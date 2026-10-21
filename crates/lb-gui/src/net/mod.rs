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

use eframe::egui::ColorImage;
use serde_json::Value;

use lb_core::graphics::GraphicsConfig;
use lb_core::media::MediaItem;
use lb_core::playlist::Playlist;
use lb_core::rules::RuleSet;
use lb_core::rundown::Rundown;
use lb_core::schedule::ScheduleSet;

use crate::model::StatusView;

/// Results delivered to the UI thread.
pub enum Msg {
    Channels(Vec<String>),
    Status(Box<StatusView>),
    Rundown(Rundown),
    Plan(Rundown),
    Library(Vec<MediaItem>),
    Playlists(Vec<Playlist>),
    Schedules(ScheduleSet),
    Rules(RuleSet),
    Graphics(GraphicsConfig),
    Stats(Value),
    Preview(ColorImage),
    Event(Value),
    /// A command / save succeeded (message for the user).
    Done(String),
    Error(String),
    Connection(bool),
}

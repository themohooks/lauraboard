//! Real-time events (WebSocket stream, logs, notifications).

use serde::{Deserialize, Serialize};

use lb_core::WallTime;
use lb_core::history::AsRunRecord;
use lb_core::rundown::RundownItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    Started,
    Stopped,
    ItemStarted { item: RundownItem },
    ItemEnded { record: AsRunRecord },
    RundownChanged { items: usize, warnings: usize },
    ModeChanged { manual: bool },
    ScheduleSwitched { schedule: Option<String> },
    EncoderStarted { codec: String },
    EncoderStopped { error: Option<String> },
    GraphicsChanged,
    Message { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub channel: String,
    pub at: WallTime,
    pub level: Level,
    #[serde(flatten)]
    pub kind: EventKind,
}

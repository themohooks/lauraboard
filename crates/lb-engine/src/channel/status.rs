//! Published channel status (consumed by API, CLI and GUI).

use serde::{Deserialize, Serialize};

use lb_core::graphics::ActiveGraphic;
use lb_core::rundown::RundownItem;
use lb_core::{Ms, WallTime};
use lb_media::progress::EncoderStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    #[default]
    Stopped,
    Starting,
    OnAir,
    /// Running but the encoder is down and being restarted.
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OnAirInfo {
    pub item: RundownItem,
    /// Actual start (wall clock).
    pub started: WallTime,
    pub elapsed_ms: Ms,
    pub remaining_ms: Ms,
    /// Actual start minus planned start (positive = late).
    pub deviation_ms: Ms,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OutputStatus {
    pub running: bool,
    pub video_codec: String,
    pub restarts: u32,
    pub stats: EncoderStats,
    #[serde(default)]
    pub last_error: Option<String>,
    pub targets: Vec<String>,
    /// Network outputs that are failing right now (the encoder keeps
    /// running and retries them).
    #[serde(default)]
    pub problems: Vec<String>,
    /// Recording to files, while it is on and the encoder runs.
    #[serde(default)]
    pub recording: Option<super::recordings::RecordingStatus>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelStatus {
    pub channel: String,
    pub name: String,
    /// Version of the channel avatar (`GET /channels/{id}/avatar.png`);
    /// none without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<u64>,
    /// Starts airing when the program starts.
    #[serde(default)]
    pub autostart: bool,
    pub state: RunState,
    /// Schedule paused, operator in control.
    pub manual: bool,
    pub force_schedule: Option<String>,
    pub now: Option<WallTime>,
    pub on_air: Option<OnAirInfo>,
    pub next: Option<RundownItem>,
    /// Next main item of a different block.
    pub next_program: Option<RundownItem>,
    pub time_to_next_program_ms: Option<Ms>,
    /// The programme on air: aired, current and planned items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<lb_core::graphics::ProgramClock>,
    pub manual_queue: usize,
    pub output: OutputStatus,
    pub graphics: Vec<ActiveGraphic>,
    pub plan_warnings: usize,
    pub last_error: Option<String>,
}

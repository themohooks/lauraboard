//! Client-side cache of server data. The status mirror is defined here
//! (not imported from the engine) so the GUI depends only on the API.

use std::collections::VecDeque;

use serde::Deserialize;
use serde_json::Value;

use lb_core::graphics::{ActiveGraphic, GraphicsConfig};
use lb_core::media::MediaItem;
use lb_core::playlist::Playlist;
use lb_core::rules::RuleSet;
use lb_core::rundown::{Rundown, RundownItem};
use lb_core::schedule::ScheduleSet;
use lb_core::{Ms, WallTime};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OnAirView {
    pub item: Option<RundownItem>,
    #[serde(default)]
    pub elapsed_ms: Ms,
    #[serde(default)]
    pub remaining_ms: Ms,
    #[serde(default)]
    pub deviation_ms: Ms,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EncoderStatsView {
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub bitrate_kbps: f64,
    #[serde(default)]
    pub speed: f64,
    #[serde(default)]
    pub drop_frames: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OutputView {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub video_codec: String,
    #[serde(default)]
    pub restarts: u32,
    #[serde(default)]
    pub stats: EncoderStatsView,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StatusView {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub manual: bool,
    #[serde(default)]
    pub force_schedule: Option<String>,
    #[serde(default)]
    pub now: Option<WallTime>,
    #[serde(default)]
    pub on_air: Option<OnAirView>,
    #[serde(default)]
    pub next: Option<RundownItem>,
    #[serde(default)]
    pub next_program: Option<RundownItem>,
    #[serde(default)]
    pub time_to_next_program_ms: Option<Ms>,
    #[serde(default)]
    pub manual_queue: usize,
    #[serde(default)]
    pub output: OutputView,
    #[serde(default)]
    pub graphics: Vec<ActiveGraphic>,
    #[serde(default)]
    pub plan_warnings: usize,
    #[serde(default)]
    pub last_error: Option<String>,
}

/// One line of the event log.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub at: String,
    pub level: String,
    pub text: String,
}

impl LogLine {
    pub fn from_event(v: &Value) -> Self {
        let kind = v["type"].as_str().unwrap_or("event");
        let text = match kind {
            "item_started" => format!("▶ {}", v["item"]["title"].as_str().unwrap_or("")),
            "item_ended" => format!("■ {} ({})", v["record"]["title"].as_str().unwrap_or(""), v["record"]["status"].as_str().unwrap_or("")),
            "message" => v["text"].as_str().unwrap_or("").to_string(),
            "encoder_stopped" => format!("encoder stopped: {}", v["error"].as_str().unwrap_or("")),
            "encoder_started" => format!("encoder started ({})", v["codec"].as_str().unwrap_or("")),
            "mode_changed" => format!("manual mode: {}", v["manual"]),
            "schedule_switched" => format!("schedule: {}", v["schedule"].as_str().unwrap_or("auto")),
            "rundown_changed" => format!("rundown updated ({} items, {} warnings)", v["items"], v["warnings"]),
            other => other.replace('_', " "),
        };
        LogLine {
            at: v["at"].as_str().map(|s| s.get(11..19).unwrap_or(s).to_string()).unwrap_or_default(),
            level: v["level"].as_str().unwrap_or("info").to_string(),
            text: format!("[{}] {text}", v["channel"].as_str().unwrap_or("")),
        }
    }
}

/// One channel of the project (from `GET /channels`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelEntry {
    pub id: String,
    pub name: String,
    pub state: String,
    pub autostart: bool,
}

impl ChannelEntry {
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        Some(ChannelEntry {
            id: v["channel"].as_str()?.to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            state: v["state"].as_str().unwrap_or("stopped").to_string(),
            autostart: v["autostart"].as_bool().unwrap_or(false),
        })
    }

    /// Name for lists: the display name, or the id when it has none.
    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() { &self.id } else { &self.name }
    }
}

/// Everything the GUI knows about the selected channel.
#[derive(Default)]
pub struct Model {
    /// Channel ids, in list order.
    pub channels: Vec<String>,
    pub channel_info: Vec<ChannelEntry>,
    pub channel: Option<String>,
    pub online: bool,
    pub status: StatusView,
    pub rundown: Rundown,
    pub library: Vec<MediaItem>,
    pub playlists: Vec<Playlist>,
    pub schedules: ScheduleSet,
    pub rules: RuleSet,
    pub graphics: GraphicsConfig,
    pub log: VecDeque<LogLine>,
}

impl Model {
    pub fn push_log(&mut self, l: LogLine) {
        self.log.push_front(l);
        self.log.truncate(1000);
    }

    pub fn ch(&self) -> String {
        self.channel.clone().unwrap_or_default()
    }
}

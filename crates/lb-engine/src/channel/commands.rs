//! Commands accepted by a channel actor.

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use lb_core::graphics::Manual;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Start broadcasting (encoder + playout).
    Start,
    /// Stop broadcasting gracefully (state is saved).
    Stop,
    /// Skip the current item.
    Skip,
    /// Play the current item again from the beginning.
    Restart,
    /// Jump inside the current item: to `to_ms` from its start, or by
    /// `by_ms` (negative: back) from the current position.
    Seek {
        #[serde(default)]
        to_ms: Option<i64>,
        #[serde(default)]
        by_ms: Option<i64>,
    },
    /// Pause (true) or resume (false) the schedule.
    Manual { enabled: bool },
    /// Interrupt and play a media item now.
    PlayNow { media: String },
    /// Queue a media item to play after the current one.
    Queue { media: String },
    /// Clear the manual queue.
    ClearQueue,
    /// Unscheduled ad break; `now` interrupts the current item.
    AdBreak {
        ad_break: String,
        #[serde(default)]
        now: bool,
    },
    /// Show / hide a graphic element (overrides rules).
    Graphic { element: String, action: Manual },
    /// Remove a manual graphic override (rules decide again).
    GraphicAuto { element: String },
    /// Set (or remove with `None`) a template variable such as `ticker`.
    SetVar { key: String, value: Option<String> },
    /// Operator-selected schedule (`None` = by activation time).
    SwitchSchedule { schedule: Option<String> },
    /// Configuration / data changed: reload and replan.
    Reload,
    /// Stop broadcasting and end the actor (channel deleted or renamed).
    /// Not accepted from API clients.
    Shutdown,
}

pub type Reply = Result<String, String>;

pub struct Request {
    pub cmd: Command,
    pub reply: Option<oneshot::Sender<Reply>>,
}

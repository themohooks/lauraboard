//! Planned on-air timeline produced by the planner.

use serde::{Deserialize, Serialize};

use crate::media::MediaKind;
use crate::playlist::Cursor;
use crate::{Ms, WallTime, add_ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Main content of a block.
    Main,
    /// Block intro/outro.
    BlockBookend,
    /// Inserted by an interstitial rule.
    Rule,
    /// Gap filler.
    Filler,
    /// Synthetic slate / standby image.
    Slate,
    /// Manually inserted by the operator.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RundownItem {
    pub uid: String,
    /// Media id; empty for slate.
    pub media: String,
    pub title: String,
    pub kind: MediaKind,
    pub origin: Origin,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub block: Option<String>,
    #[serde(default)]
    pub block_title: Option<String>,
    /// `date:block_id` of the block instance.
    #[serde(default)]
    pub block_key: Option<String>,
    #[serde(default)]
    pub playlist: Option<String>,
    #[serde(default)]
    pub rule: Option<String>,
    pub start: WallTime,
    /// Planned on-air duration.
    pub duration_ms: Ms,
    /// Start offset into the media (in point or resume).
    #[serde(default)]
    pub seek_ms: Ms,
    /// Duration was shortened by planning (explicitly allowed).
    #[serde(default)]
    pub truncated: bool,
    /// May be dropped to meet fixed times.
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub priority: i32,
    /// For the first item of a fixed block: the nominal fixed start.
    #[serde(default)]
    pub fixed_start: Option<WallTime>,
    #[serde(default)]
    pub fade_in_ms: Ms,
    #[serde(default)]
    pub fade_out_ms: Ms,
    /// Playlist cursor after this item airs.
    #[serde(default)]
    pub cursor_after: Option<(String, Cursor)>,
    #[serde(default)]
    pub skip_on_manual: bool,
}

impl RundownItem {
    pub fn end(&self) -> WallTime {
        add_ms(self.start, self.duration_ms)
    }

    /// Delay of a fixed-start item versus its nominal time.
    pub fn fixed_deviation_ms(&self) -> Option<Ms> {
        self.fixed_start.map(|f| crate::diff_ms(self.start, f))
    }

    pub fn is_slate(&self) -> bool {
        self.origin == Origin::Slate
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    Gap,
    Overlap,
    LateStart,
    MissingMedia,
    EmptySource,
    Truncated,
    Dropped,
    Config,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanWarning {
    pub kind: WarningKind,
    pub at: Option<WallTime>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Rundown {
    pub items: Vec<RundownItem>,
    pub warnings: Vec<PlanWarning>,
}

impl Rundown {
    pub fn end(&self) -> Option<WallTime> {
        self.items.last().map(|i| i.end())
    }

    /// Item airing at `t`.
    pub fn at(&self, t: WallTime) -> Option<(usize, &RundownItem)> {
        self.items.iter().enumerate().find(|(_, i)| i.start <= t && t < i.end())
    }

    pub fn total_ms(&self) -> Ms {
        self.items.iter().map(|i| i.duration_ms).sum()
    }

    /// Next item (after `idx`) that starts a different block.
    pub fn next_block_start(&self, idx: usize) -> Option<&RundownItem> {
        let cur = self.items.get(idx)?.block_key.clone();
        self.items[idx + 1..].iter().find(|i| i.block_key != cur && i.origin == Origin::Main)
    }

    /// Shift all items from `from` by `delta` ms (used by the engine when the
    /// actual start differs from the plan).
    pub fn shift_from(&mut self, from: usize, delta: Ms) {
        for it in self.items.iter_mut().skip(from) {
            it.start = add_ms(it.start, delta);
        }
    }
}

//! Play history used by the planner (recent-play exclusion, ad caps) and
//! the as-run log record format.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::media::MediaKind;
use crate::{Ms, WallTime};

/// Read-only view of what was aired.
pub trait PlayHistory {
    fn last_played(&self, media: &str) -> Option<WallTime>;
    fn count_on(&self, media: &str, date: NaiveDate) -> u32;
}

/// Simple in-memory history.
#[derive(Debug, Clone, Default)]
pub struct MemHistory {
    plays: HashMap<String, Vec<WallTime>>,
}

impl MemHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, media: &str, at: WallTime) {
        let v = self.plays.entry(media.to_string()).or_default();
        v.push(at);
        v.sort();
    }

    /// Forget plays older than `before`.
    pub fn prune(&mut self, before: WallTime) {
        for v in self.plays.values_mut() {
            v.retain(|t| *t >= before);
        }
        self.plays.retain(|_, v| !v.is_empty());
    }

    pub fn from_records<'a>(recs: impl IntoIterator<Item = &'a AsRunRecord>) -> Self {
        let mut h = MemHistory::new();
        for r in recs {
            if r.status != AsRunStatus::Skipped {
                h.record(&r.media, r.started);
            }
        }
        h
    }
}

impl PlayHistory for MemHistory {
    fn last_played(&self, media: &str) -> Option<WallTime> {
        self.plays.get(media).and_then(|v| v.last().copied())
    }
    fn count_on(&self, media: &str, date: NaiveDate) -> u32 {
        self.plays.get(media).map(|v| v.iter().filter(|t| t.date() == date).count() as u32).unwrap_or(0)
    }
}

/// History overlay: base history plus items planned in the current pass.
pub struct Overlay<'a> {
    pub base: &'a dyn PlayHistory,
    pub planned: MemHistory,
}

impl<'a> Overlay<'a> {
    pub fn new(base: &'a dyn PlayHistory) -> Self {
        Overlay { base, planned: MemHistory::new() }
    }
}

impl PlayHistory for Overlay<'_> {
    fn last_played(&self, media: &str) -> Option<WallTime> {
        match (self.base.last_played(media), self.planned.last_played(media)) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }
    fn count_on(&self, media: &str, date: NaiveDate) -> u32 {
        self.base.count_on(media, date) + self.planned.count_on(media, date)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsRunStatus {
    /// Played to the planned end.
    Completed,
    /// Stopped early (skip, stop, error mid-way).
    Interrupted,
    /// Not played at all (missing file, dropped, skipped by operator).
    Skipped,
    /// Planned truncation.
    Truncated,
}

/// One line of the as-run log (`asrun/YYYY-MM-DD.jsonl`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AsRunRecord {
    pub channel: String,
    pub uid: String,
    pub media: String,
    pub title: String,
    pub kind: MediaKind,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub block: Option<String>,
    #[serde(default)]
    pub rule: Option<String>,
    pub planned_start: WallTime,
    pub started: WallTime,
    pub ended: WallTime,
    pub planned_duration_ms: Ms,
    pub played_ms: Ms,
    pub status: AsRunStatus,
    #[serde(default)]
    pub note: String,
}

impl AsRunRecord {
    /// started - planned_start (positive = late).
    pub fn deviation_ms(&self) -> Ms {
        crate::diff_ms(self.started, self.planned_start)
    }
}

//! Timing of the programme on air, for the "time to the end of the
//! programme" countdown and the programme progress bar. A programme is
//! the block instance of the item on air (its items, interstitials
//! included); without a block it is the item itself.

use serde::{Deserialize, Serialize};

use crate::Ms;
use crate::media::MediaKind;

/// What the programme is made of: aired items, the item on air, the
/// planned rest. Elements count only the kinds they do not exclude (say,
/// a countdown that ignores the ads inside the programme).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgramClock {
    /// Already aired items: kind, played length.
    #[serde(default)]
    pub past: Vec<(MediaKind, Ms)>,
    /// The item on air: kind and planned length.
    pub current: Option<(MediaKind, Ms)>,
    /// Planned items after it.
    #[serde(default)]
    pub future: Vec<(MediaKind, Ms)>,
}

impl ProgramClock {
    /// (elapsed, total) of the counted kinds when `current_remaining_ms`
    /// of the item on air is left.
    pub fn span(&self, exclude: &[MediaKind], current_remaining_ms: Ms) -> (Ms, Ms) {
        let counted = |k: &MediaKind| !exclude.contains(k);
        let past: Ms = self.past.iter().filter(|(k, _)| counted(k)).map(|(_, ms)| *ms).sum();
        let rest: Ms = self.future.iter().filter(|(k, _)| counted(k)).map(|(_, ms)| *ms).sum();
        let (cur_done, cur_total) = match self.current {
            Some((k, d)) if counted(&k) => ((d - current_remaining_ms.max(0)).clamp(0, d.max(0)), d.max(0)),
            _ => (0, 0),
        };
        (past + cur_done, past + cur_total + rest)
    }

    /// Time left in the programme (counted kinds only).
    pub fn remaining(&self, exclude: &[MediaKind], current_remaining_ms: Ms) -> Ms {
        let (done, total) = self.span(exclude, current_remaining_ms);
        (total - done).max(0)
    }

    /// Share of the programme that has aired, 0…1.
    pub fn progress(&self, exclude: &[MediaKind], current_remaining_ms: Ms) -> f32 {
        let (done, total) = self.span(exclude, current_remaining_ms);
        if total <= 0 { 0.0 } else { (done as f64 / total as f64).clamp(0.0, 1.0) as f32 }
    }
}

/// What a progress bar follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProgressTarget {
    /// The programme (block) on air.
    #[default]
    Program,
    /// The item on air.
    Item,
}

/// Which way a progress bar fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FillDirection {
    #[default]
    LeftToRight,
    RightToLeft,
    BottomToTop,
    TopToBottom,
}

impl FillDirection {
    pub const ALL: [FillDirection; 4] = [FillDirection::LeftToRight, FillDirection::RightToLeft, FillDirection::BottomToTop, FillDirection::TopToBottom];
}

/// How a countdown shows the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeFormat {
    /// `1:05:09`, `05:09` under an hour.
    #[default]
    Auto,
    /// Always `01:05:09`.
    HhMmSs,
    /// Minutes and seconds, `65:09`.
    MmSs,
    /// Whole minutes, rounded up: `66 мин`.
    Minutes,
}

impl TimeFormat {
    pub const ALL: [TimeFormat; 4] = [TimeFormat::Auto, TimeFormat::HhMmSs, TimeFormat::MmSs, TimeFormat::Minutes];

    /// `ms` as text (a partial second counts as a whole one, so the
    /// countdown reaches 0 at the very end).
    pub fn format(self, ms: Ms) -> String {
        let s = (ms.max(0) + 999) / 1000;
        let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
        match self {
            TimeFormat::Auto if h > 0 => format!("{h}:{m:02}:{sec:02}"),
            TimeFormat::Auto => format!("{m:02}:{sec:02}"),
            TimeFormat::HhMmSs => format!("{h:02}:{m:02}:{sec:02}"),
            TimeFormat::MmSs => format!("{:02}:{sec:02}", s / 60),
            TimeFormat::Minutes => format!("{} мин", (s + 59) / 60),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> ProgramClock {
        ProgramClock {
            past: vec![(MediaKind::Program, 600_000), (MediaKind::Ad, 120_000)],
            current: Some((MediaKind::Program, 600_000)),
            future: vec![(MediaKind::Jingle, 10_000), (MediaKind::Program, 300_000)],
        }
    }

    #[test]
    fn counts_everything_by_default() {
        let c = clock();
        // 4 of the current 10 minutes left
        assert_eq!(c.span(&[], 240_000), (600_000 + 120_000 + 360_000, 600_000 + 120_000 + 600_000 + 10_000 + 300_000));
        assert_eq!(c.remaining(&[], 240_000), 240_000 + 10_000 + 300_000);
    }

    #[test]
    fn excluded_kinds_do_not_count() {
        let c = clock();
        assert_eq!(c.remaining(&[MediaKind::Ad, MediaKind::Jingle], 240_000), 240_000 + 300_000);
        let p = c.progress(&[MediaKind::Ad, MediaKind::Jingle], 240_000);
        assert!((p - 960.0 / 1500.0).abs() < 1e-4, "{p}");
        // an excluded item on air: the clock stands still
        let ad = ProgramClock { current: Some((MediaKind::Ad, 60_000)), ..clock() };
        assert_eq!(ad.remaining(&[MediaKind::Ad], 10_000), ad.remaining(&[MediaKind::Ad], 50_000));
    }

    #[test]
    fn time_formats() {
        assert_eq!(TimeFormat::Auto.format(3_909_000), "1:05:09");
        assert_eq!(TimeFormat::Auto.format(309_000), "05:09");
        assert_eq!(TimeFormat::HhMmSs.format(309_000), "00:05:09");
        assert_eq!(TimeFormat::MmSs.format(3_909_000), "65:09");
        assert_eq!(TimeFormat::Minutes.format(3_909_000), "66 мин");
        assert_eq!(TimeFormat::Auto.format(1), "00:01");
        assert_eq!(TimeFormat::Auto.format(0), "00:00");
    }
}

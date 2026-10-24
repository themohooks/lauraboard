//! Fitting content between fixed start times: gap fill, dropping optional
//! items, truncation (only when allowed) and late-start reporting.

use crate::rundown::{Origin, RundownItem, WarningKind};
use crate::schedule::LatePolicy;
use crate::{WallTime, diff_ms, fmt_ms};

use super::MIN_TRUNCATED_PLAY_MS;
use super::context::{Inst, Planner};

fn end_of(v: &[RundownItem]) -> Option<WallTime> {
    v.last().map(|i| i.end())
}

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// Make the content in `out[window_start..]` end exactly at `fixed`.
    /// Returns the actual start time for the fixed block.
    pub(super) fn resolve_window(&mut self, window_start: usize, t: WallTime, fixed: WallTime, prev: Option<&Inst<'a>>) -> WallTime {
        let mut end = t;
        if end > fixed {
            end = self.shrink_window(window_start, fixed, prev);
        }
        if end < fixed {
            if prev.is_none_or(|p| p.block.fill_gap) {
                if !(self.inp.rules.filler.scenes_first && self.has_filler_scene()) {
                    end = self.fill_gap(end, fixed, prev);
                }
                end = self.scene_gap(end, fixed, prev);
            }
            if end < fixed {
                let gap = diff_ms(fixed, end);
                if gap > 60_000 {
                    self.warn(WarningKind::Gap, Some(end), format!("unfilled gap of {} before {}", fmt_ms(gap), fixed.time()));
                }
                self.slate(end, gap, prev);
                end = fixed;
            }
        }
        if end > fixed {
            let late = diff_ms(end, fixed);
            self.warn(WarningKind::LateStart, Some(fixed), format!("block at {} starts {} late", fixed.time(), fmt_ms(late)));
        }
        end
    }

    /// Overflow: drop optional items, then truncate if allowed. Returns new end.
    fn shrink_window(&mut self, window_start: usize, fixed: WallTime, prev: Option<&Inst<'a>>) -> WallTime {
        let mut overflow = diff_ms(end_of(&self.out).unwrap_or(fixed), fixed);
        overflow = self.drop_optional(window_start, fixed, overflow);
        if overflow > 0 {
            let allowed = prev.is_some_and(|p| p.block.allow_truncate && p.block.late_policy != LatePolicy::Finish);
            if allowed {
                self.truncate_last_main(window_start, overflow);
            }
        }
        end_of(&self.out).unwrap_or(fixed)
    }

    /// Remove optional interstitials (lowest priority, latest first) until
    /// `overflow` is absorbed. Returns the remaining overflow.
    fn drop_optional(&mut self, window_start: usize, fixed: WallTime, overflow: i64) -> i64 {
        let mut cands: Vec<(i32, usize)> = self.out[window_start..]
            .iter()
            .enumerate()
            .filter(|(_, it)| it.optional && it.origin != Origin::Main)
            .map(|(i, it)| (it.priority, window_start + i))
            .collect();
        cands.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let mut drop: Vec<usize> = Vec::new();
        let mut gained = 0;
        for (_, idx) in cands {
            if gained >= overflow {
                break;
            }
            gained += self.out[idx].duration_ms;
            drop.push(idx);
        }
        if drop.is_empty() {
            return overflow;
        }
        drop.sort_unstable_by(|a, b| b.cmp(a));
        for idx in drop {
            let it = self.out.remove(idx);
            self.warn(WarningKind::Dropped, Some(it.start), format!("dropped optional '{}' to keep {}", it.title, fixed.time()));
        }
        self.retime(window_start);
        diff_ms(end_of(&self.out).unwrap_or(fixed), fixed)
    }

    /// Shorten the last main item of the window by `overflow` (explicitly
    /// allowed by the block).
    fn truncate_last_main(&mut self, window_start: usize, overflow: i64) {
        let Some(i) = (window_start..self.out.len()).rev().find(|i| self.out[*i].origin == Origin::Main) else { return };
        let keep = self.out[i].duration_ms - overflow;
        if keep < MIN_TRUNCATED_PLAY_MS {
            return;
        }
        self.out[i].duration_ms = keep;
        self.out[i].truncated = true;
        let (title, start) = (self.out[i].title.clone(), self.out[i].start);
        self.warn(WarningKind::Truncated, Some(start), format!("'{title}' truncated by {}", fmt_ms(overflow)));
        self.retime(window_start);
    }

    /// Recompute start times sequentially from `from_idx`.
    pub(super) fn retime(&mut self, from_idx: usize) {
        if from_idx >= self.out.len() {
            return;
        }
        let mut t = if from_idx > 0 { self.out[from_idx - 1].end() } else { self.out[from_idx].start };
        for it in &mut self.out[from_idx..] {
            it.start = t;
            t = it.end();
        }
    }
}

//! Filling a single block instance with main content and rule insertions.

use crate::media::MediaItem;
use crate::rules::{InsertionRule, Trigger};
use crate::rundown::WarningKind;
use crate::schedule::LatePolicy;
use crate::{Ms, WallTime, add_ms, diff_ms, fmt_ms};

use super::context::{Inst, MatchCtxOwned, Planner, Src};
use super::source::block_playlist;
use super::{MAX_ITEMS, MIN_TRUNCATED_PLAY_MS};

/// Limits for one block fill.
pub(super) struct BlockBounds {
    /// Next fixed start (cannot be passed without the late policy).
    pub hard_end: Option<WallTime>,
    /// `max_duration` end: no new main item starts after it.
    pub soft_end: Option<WallTime>,
    pub max_items: Option<usize>,
    /// Planning horizon.
    pub until: WallTime,
}

/// Outcome of trying to place the next main item.
enum Step {
    Placed,
    /// Placed but truncated: block ends.
    PlacedLast,
    Stop,
}

/// Running counters while filling a block.
struct Progress<'a> {
    src: Src<'a>,
    count: usize,
    last_main: Option<String>,
    block_start: WallTime,
    alt_tries: u32,
}

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// Fill one block instance starting at `t`. Returns the end time.
    pub(super) fn fill_block(
        &mut self,
        inst: &Inst<'a>,
        mut t: WallTime,
        hard_end: Option<WallTime>,
        next_is_follow: bool,
        items_done: usize,
        until: WallTime,
    ) -> WallTime {
        let b = inst.block;
        let src = self.src_for(b, items_done);
        match src {
            Src::Empty => {
                self.warn(WarningKind::EmptySource, Some(t), format!("block '{}' has no valid source", b.id));
                return t;
            }
            Src::AdBreak { brk } => {
                let items = self.build_ad_break(brk, t, None, false, 0);
                return self.push_ad_items(items, t, Some(inst), false);
            }
            _ => {}
        }
        let bounds = BlockBounds {
            hard_end,
            soft_end: b.max_duration.map(|d| add_ms(t, d.ms())),
            max_items: self.max_items(inst, &src, next_is_follow),
            until,
        };
        // before the intro: a time rule due at the block's start time fires
        // in it even when the intro moves the first item past that time
        let block_start = t;
        if items_done == 0 {
            self.open_block(inst, &mut t);
        }
        let mut pr = Progress { src, count: items_done, last_main: None, block_start, alt_tries: 8 };
        loop {
            if !self.can_continue(&bounds, &pr, t) {
                break;
            }
            match self.place_next_main(inst, &mut pr, &mut t, &bounds) {
                Step::Placed => {}
                Step::PlacedLast | Step::Stop => break,
            }
        }
        self.close_block(inst, &mut t);
        t
    }

    /// When a block is followed by follow-blocks and has no explicit limit,
    /// it plays one pass of its source so the following blocks get airtime.
    fn max_items(&self, inst: &Inst<'a>, src: &Src<'a>, next_is_follow: bool) -> Option<usize> {
        let b = inst.block;
        b.max_items.or(if next_is_follow && b.max_duration.is_none() {
            Some(match src {
                Src::Playlist { pl, .. } => pl.entries.iter().filter(|e| !e.disabled).count(),
                Src::Media { items, .. } => items.len(),
                _ => 1,
            })
        } else {
            None
        })
    }

    fn can_continue(&self, bd: &BlockBounds, pr: &Progress<'a>, t: WallTime) -> bool {
        t < bd.until
            && self.out.len() < MAX_ITEMS
            && bd.max_items.is_none_or(|m| pr.count < m)
            && bd.soft_end.is_none_or(|s| t < s)
            && bd.hard_end.is_none_or(|h| t < h)
    }

    /// Intro and `block_start` rules.
    fn open_block(&mut self, inst: &Inst<'a>, t: &mut WallTime) {
        let b = inst.block;
        if let Some(intro) = &b.intro {
            self.push_bookend(intro, t, inst);
        }
        if b.rules {
            let ctx = self.block_ctx(b, *t);
            for r in self.firing_rules(|r| matches!(r.trigger, Trigger::BlockStart), &ctx) {
                self.apply_rule(r, t, Some(inst));
            }
        }
    }

    /// `block_end` rules and outro.
    fn close_block(&mut self, inst: &Inst<'a>, t: &mut WallTime) {
        let b = inst.block;
        if b.rules {
            let ctx = self.block_ctx(b, *t);
            for r in self.firing_rules(|r| matches!(r.trigger, Trigger::BlockEnd), &ctx) {
                self.apply_rule(r, t, Some(inst));
            }
        }
        if let Some(outro) = &b.outro {
            self.push_bookend(outro, t, inst);
        }
    }

    /// Try to place the next main item (with its `before_category` rules).
    fn place_next_main(&mut self, inst: &Inst<'a>, pr: &mut Progress<'a>, t: &mut WallTime, bd: &BlockBounds) -> Step {
        let b = inst.block;
        let Some((m, next_src)) = self.peek_src(&pr.src, b, *t, pr.last_main.as_deref(), None) else { return Step::Stop };
        let cat = self.effective_category(m, b, &pr.src);
        let pl_id = if let Src::Playlist { pl, .. } = &pr.src { Some(pl.id.clone()) } else { None };

        let pre = self.before_rules(inst, m, cat.as_deref(), pl_id.clone(), *t);
        let mark = self.out.len();
        let mut t2 = *t;
        for r in &pre {
            self.apply_rule(r, &mut t2, Some(inst));
        }
        let dur = m.play_duration();
        let mut planned = dur;
        let mut truncated = false;
        // Timed rules at this boundary precede the main item. Conditions
        // are evaluated at the scheduled instant, with the upcoming item.
        if b.rules {
            let ctx = self.main_ctx(inst, m, cat.clone(), pl_id.clone(), t2);
            for r in self
                .firing_rules(|r| r.time_conflict == crate::rules::TimeConflict::Truncate && r.trigger.occurrence(t2, t2).is_some(), &ctx)
            {
                if self.st.rule_last_fired.get(&r.id).is_none_or(|last| *last < t2) {
                    self.apply_rule(r, &mut t2, Some(inst));
                }
            }
        }
        // Materialise the earliest interrupting insertion before cutting.
        // A failed probability check or empty source must never cut a clip.
        let mut timed = Vec::new();
        if b.rules {
            let end = bd.hard_end.map_or(crate::add_ms(t2, planned), |h| h.min(crate::add_ms(t2, planned)));
            let mut from = crate::add_ms(t2, 1);
            while from <= end {
                let at = self
                    .inp
                    .rules
                    .rules
                    .iter()
                    .filter(|r| r.enabled && r.time_conflict == crate::rules::TimeConflict::Truncate)
                    .filter_map(|r| r.trigger.occurrence(from, end))
                    .min();
                let Some(at) = at else { break };
                let ctx = self.main_ctx(inst, m, cat.clone(), pl_id.clone(), at);
                let mark = self.out.len();
                let mut after = at;
                for r in self.firing_rules(
                    |r| r.time_conflict == crate::rules::TimeConflict::Truncate && r.trigger.occurrence(at, at).is_some(),
                    &ctx,
                ) {
                    self.apply_rule(r, &mut after, Some(inst));
                }
                timed = self.out.split_off(mark);
                if !timed.is_empty() {
                    planned = diff_ms(at, t2);
                    truncated = planned < dur;
                    break;
                }
                from = crate::add_ms(at, 1);
            }
        }
        let mut block_truncated = false;
        if let Some(h) = bd.hard_end {
            let room = diff_ms(h, t2);
            if planned > room {
                let can_cut = b.allow_truncate && room >= MIN_TRUNCATED_PLAY_MS;
                match b.late_policy {
                    LatePolicy::Finish => {}
                    LatePolicy::Truncate if can_cut => {
                        planned = room;
                        truncated = true;
                        block_truncated = true;
                    }
                    _ => {
                        // does not fit: undo pre-rules and maybe try a shorter item
                        self.out.truncate(mark);
                        let room_now = diff_ms(h, *t);
                        if self.place_shorter_alternative(inst, pr, t, room_now) {
                            return Step::Placed;
                        }
                        if pr.count == 0 {
                            self.warn(
                                WarningKind::Overlap,
                                Some(*t),
                                format!("'{}' ({}) does not fit before {}", m.title, fmt_ms(dur), h.time()),
                            );
                        }
                        return Step::Stop;
                    }
                }
            }
        }
        *t = t2;
        let cursor_after = match &next_src {
            Src::Playlist { pl, cursor } => Some((pl.id.clone(), *cursor)),
            _ => None,
        };
        self.push_main(m, t, inst, cat, pl_id, cursor_after, planned, truncated);
        if block_truncated {
            self.warn(WarningKind::Truncated, Some(*t), format!("'{}' truncated to fit {}", m.title, bd.hard_end.unwrap().time()));
        }
        for (index, mut item) in timed.into_iter().enumerate() {
            item.start = *t;
            if index == 0 {
                item.fixed_start = Some(*t);
            }
            item.optional = false;
            *t = item.end();
            self.out.push(item);
        }
        let pass_end = self.pass_ended(&next_src);
        pr.src = next_src;
        pr.count += 1;
        pr.last_main = Some(m.id.clone());
        self.after_main(inst, m, t, pr.count, pr.block_start, pass_end.as_deref());
        if block_truncated { Step::PlacedLast } else { Step::Placed }
    }

    /// Category sources may substitute a shorter item that fits.
    fn place_shorter_alternative(&mut self, inst: &Inst<'a>, pr: &mut Progress<'a>, t: &mut WallTime, room: Ms) -> bool {
        if !matches!(pr.src, Src::Category { .. }) || pr.alt_tries == 0 || room <= 0 {
            return false;
        }
        pr.alt_tries -= 1;
        let Some((alt, _)) = self.peek_src(&pr.src, inst.block, *t, pr.last_main.as_deref(), Some(room)) else { return false };
        let dur = alt.play_duration();
        if dur > room {
            return false;
        }
        let cat = self.effective_category(alt, inst.block, &pr.src);
        self.push_main(alt, t, inst, cat, None, None, dur, false);
        pr.count += 1;
        pr.last_main = Some(alt.id.clone());
        self.after_main(inst, alt, t, pr.count, pr.block_start, None);
        true
    }

    /// The playlist whose pass the source position just completed.
    fn pass_ended(&self, src: &Src<'a>) -> Option<String> {
        let Src::Playlist { pl, cursor } = src else { return None };
        (cursor.pos >= pl.order(self.inp.seed, cursor.cycle).len()).then(|| pl.id.clone())
    }

    fn main_ctx(&self, inst: &Inst<'a>, m: &MediaItem, cat: Option<String>, playlist: Option<String>, at: WallTime) -> MatchCtxOwned {
        MatchCtxOwned { at, category: cat, playlist, block: Some(inst.block.id.clone()), media: Some(m.id.clone()), kind: Some(m.kind) }
    }

    fn before_rules(&self, inst: &Inst<'a>, m: &MediaItem, cat: Option<&str>, pl: Option<String>, at: WallTime) -> Vec<&'a InsertionRule> {
        if !inst.block.rules {
            return vec![];
        }
        let ctx = self.main_ctx(inst, m, cat.map(str::to_string), pl, at);
        self.firing_rules(
            |r| matches!(&r.trigger, Trigger::BeforeCategory { category } if cat.is_some_and(|c| c.eq_ignore_ascii_case(category))),
            &ctx,
        )
    }

    /// Rules evaluated after each main item (`pass_end`: the playlist
    /// whose last entry it was).
    fn after_main(
        &mut self,
        inst: &Inst<'a>,
        m: &'a MediaItem,
        t: &mut WallTime,
        count: usize,
        block_start: WallTime,
        pass_end: Option<&str>,
    ) {
        let b = inst.block;
        if !b.rules {
            return;
        }
        let cat = b.category.clone().or_else(|| m.category.clone());
        let ctx = self.main_ctx(inst, m, cat.clone(), block_playlist(b), *t);
        let now = *t;
        // interval checks happen before group selection, so a not-yet-due
        // rule never suppresses others of its group
        let last_fired = &self.st.rule_last_fired;
        let rules = self.firing_rules(
            |r| match &r.trigger {
                Trigger::EveryNItems { n } => *n > 0 && count.is_multiple_of(*n),
                Trigger::AfterCategory { category } => cat.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(category)),
                Trigger::EveryMinutes { minutes } => {
                    let since = last_fired.get(&r.id).copied().filter(|x| *x >= block_start).unwrap_or(block_start);
                    diff_ms(now, since) as f64 >= minutes * 60_000.0
                }
                Trigger::AfterMedia { items } => items.iter().any(|i| i == &m.id),
                Trigger::AfterPlaylist { playlist } => pass_end.is_some_and(|p| p.eq_ignore_ascii_case(playlist)),
                Trigger::AtTimes { .. } | Trigger::Hourly { .. } => {
                    if r.time_conflict == crate::rules::TimeConflict::Truncate { return false; }
                    // a time right at the block start counts for this block
                    let since = last_fired
                        .get(&r.id)
                        .copied()
                        .filter(|x| *x >= block_start)
                        .unwrap_or(block_start - chrono::Duration::milliseconds(1));
                    r.trigger.occurrence(crate::add_ms(since, 1), now).is_some()
                }
                _ => false,
            },
            &ctx,
        );
        for r in rules {
            self.apply_rule(r, t, Some(inst));
        }
    }
}

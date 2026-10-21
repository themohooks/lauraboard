//! Ad break construction: eligibility caps, ordering, fixed duration with
//! filler / slate padding, bumpers.

use crate::history::PlayHistory;
use crate::media::{MediaItem, MediaKind};
use crate::rules::{AdBreak, AdOrder};
use crate::rundown::{Origin, RundownItem, WarningKind};
use crate::{Ms, WallTime, add_ms, diff_ms};

use super::context::{Inst, Planner};

/// Shared attributes of all items of one break.
struct BreakTag {
    key: String,
    optional: bool,
    priority: i32,
}

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// Build ad break items starting at `t` (not yet pushed).
    pub(super) fn build_ad_break(
        &mut self,
        brk: &'a AdBreak,
        t: WallTime,
        rule: Option<&str>,
        optional: bool,
        priority: i32,
    ) -> Vec<RundownItem> {
        let tag = BreakTag { key: rule.unwrap_or(&brk.id).to_string(), optional, priority };
        let intro = brk.intro.as_deref().and_then(|i| self.media(i));
        let outro = brk.outro.as_deref().and_then(|i| self.media(i));
        let bookends: Ms = intro.map(|m| m.play_duration()).unwrap_or(0) + outro.map(|m| m.play_duration()).unwrap_or(0);
        let target = brk.duration.map(|d| d.ms());

        let cands = self.eligible_spots(brk, t);
        let (spots, used) = select_spots(brk, cands, target, bookends);
        if spots.is_empty() {
            self.warn(WarningKind::EmptySource, Some(t), format!("ad break '{}' has no eligible spots", brk.id));
            return vec![];
        }

        let mut out = Vec::new();
        let mut cur = t;
        if let Some(m) = intro {
            out.push(self.break_item(m, Origin::Rule, cur, &tag));
            cur = add_ms(cur, out.last().unwrap().duration_ms);
        }
        for m in &spots {
            self.hist.planned.record(&m.id, cur);
            out.push(self.break_item(m, Origin::Rule, cur, &tag));
            cur = add_ms(cur, out.last().unwrap().duration_ms);
        }
        if let Some(tg) = target {
            self.pad_break(brk, tg - used, t, &tag, &mut out);
        }
        if let Some(m) = outro {
            out.push(self.break_item(m, Origin::Rule, cur, &tag));
        }
        out
    }

    /// Ads respecting daily caps and minimum repeat interval, in break order.
    fn eligible_spots(&mut self, brk: &'a AdBreak, t: WallTime) -> Vec<&'a MediaItem> {
        let pol = &self.inp.rules.ads;
        let date = t.date();
        let mut cands: Vec<&'a MediaItem> = if brk.items.is_empty() {
            self.inp.library.by_kind_category(Some(MediaKind::Ad), brk.category.as_deref()).collect()
        } else {
            brk.items.iter().filter_map(|i| self.media(i)).filter(|m| m.enabled).collect()
        };
        cands.retain(|m| {
            m.play_duration() > 0
                && pol.max_per_day.is_none_or(|mx| self.hist.count_on(&m.id, date) < mx)
                && pol.min_interval.is_none_or(|mi| self.hist.last_played(&m.id).is_none_or(|lp| diff_ms(t, lp) >= mi.ms()))
        });
        match brk.order {
            AdOrder::Listed => {}
            AdOrder::Random => self.rng.shuffle(&mut cands),
            AdOrder::LeastPlayed => cands.sort_by_key(|m| self.hist.count_on(&m.id, date)),
        }
        cands
    }

    /// Fill the rest of a fixed-length break with fillers, then slate.
    fn pad_break(&mut self, brk: &'a AdBreak, mut rem: Ms, t: WallTime, tag: &BreakTag, out: &mut Vec<RundownItem>) {
        if rem > 0 {
            let mut fills: Vec<&'a MediaItem> = self
                .inp
                .library
                .by_kind_category(Some(MediaKind::Filler), brk.fill_category.as_deref())
                .filter(|m| m.play_duration() > 0)
                .collect();
            fills.sort_by_key(|m| std::cmp::Reverse(m.play_duration()));
            for f in fills {
                if f.play_duration() <= rem {
                    let it = self.break_item(f, Origin::Filler, t, tag);
                    rem -= it.duration_ms;
                    out.push(it);
                }
            }
        }
        if rem > 0 {
            let mut s = self.slate_item(t, rem);
            s.rule = Some(tag.key.clone());
            s.optional = tag.optional;
            out.push(s);
        }
    }

    fn break_item(&mut self, m: &'a MediaItem, origin: Origin, t: WallTime, tag: &BreakTag) -> RundownItem {
        let mut it = self.item_for(m, origin, t);
        it.rule = Some(tag.key.clone());
        it.optional = tag.optional;
        it.priority = tag.priority;
        it
    }

    /// Push built break items sequentially from `t`; returns the end time.
    pub(super) fn push_ad_items(
        &mut self,
        items: Vec<RundownItem>,
        mut t: WallTime,
        inst: Option<&Inst<'a>>,
        skip_on_manual: bool,
    ) -> WallTime {
        for mut it in items {
            self.tag_block(&mut it, inst);
            it.start = t;
            it.skip_on_manual = skip_on_manual;
            t = it.end();
            self.out.push(it);
        }
        t
    }
}

/// Take spots in order while they fit the target length and spot limit.
fn select_spots<'a>(brk: &AdBreak, cands: Vec<&'a MediaItem>, target: Option<Ms>, bookends: Ms) -> (Vec<&'a MediaItem>, Ms) {
    let mut spots = Vec::new();
    let mut used = bookends;
    for m in cands {
        if brk.max_spots.is_some_and(|mx| spots.len() >= mx) {
            break;
        }
        let d = m.play_duration();
        if target.is_some_and(|tg| used + d > tg) {
            continue;
        }
        used += d;
        spots.push(m);
    }
    (spots, used)
}

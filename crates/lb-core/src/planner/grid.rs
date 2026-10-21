//! Grid & sequential planning: walks block instances in time order.

use chrono::{Duration, NaiveDate};

use crate::rules::Trigger;
use crate::rundown::WarningKind;
use crate::schedule::{Block, BlockInstance};
use crate::{WallTime, diff_ms};

use super::context::{Inst, Planner};
use super::{MAX_ITEMS, PlanRequest};

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// Block instances covering `[from - 1 day, until + 1 day]`.
    pub(super) fn instances(&self, from: WallTime, until: WallTime) -> Vec<Inst<'a>> {
        let mut out = Vec::new();
        let mut d: NaiveDate = from.date() - Duration::days(1);
        let last = until.date() + Duration::days(1);
        while d <= last {
            for bi in self.inp.schedules.instances(d, self.inp.force_schedule) {
                let BlockInstance { date, block, fixed_start, .. } = bi;
                out.push(Inst { key: format!("{date}:{}", block.id), block, fixed: fixed_start });
            }
            d += Duration::days(1);
        }
        out
    }

    pub(super) fn plan_grid(&mut self, req: &PlanRequest) {
        let (from, until) = (req.from, req.until);
        let insts = self.instances(from, until);
        if insts.is_empty() {
            self.warn(WarningKind::Gap, Some(from), "no schedule blocks for the planning period");
            self.slate(from, diff_ms(until, from), None);
            return;
        }
        // Continue the given block, else the latest fixed block already started.
        let start_idx = req
            .current_block
            .as_ref()
            .and_then(|k| insts.iter().position(|i| &i.key == k))
            .or_else(|| insts.iter().rposition(|i| i.fixed.is_some_and(|f| f <= from)))
            .unwrap_or(0);
        let mut t = from;
        let mut items_done = if req.current_block.is_some() { req.block_items_done } else { 0 };
        let mut window_start = self.out.len();
        let mut prev_block: Option<Inst<'a>> = None;

        for i in start_idx..insts.len() {
            if t >= until || self.out.len() >= MAX_ITEMS {
                break;
            }
            let inst = insts[i].clone();
            if let Some(fixed) = inst.fixed {
                // reached a fixed block: make the previous window end at `fixed`
                if i != start_idx || fixed > t {
                    if let Some(prev) = &prev_block {
                        self.between_blocks(prev, &mut t);
                    }
                    t = self.resolve_window(window_start, t, fixed, prev_block.as_ref());
                    window_start = self.out.len();
                }
            } else if let Some(prev) = &prev_block {
                self.between_blocks(prev, &mut t);
            }
            let next_fixed = insts[i + 1..].iter().find_map(|x| x.fixed);
            let next_is_follow = insts.get(i + 1).is_some_and(|x| x.fixed.is_none());
            let first_idx = self.out.len();
            t = self.fill_block(&inst, t, next_fixed, next_is_follow, items_done, until);
            if let (Some(f), Some(first)) = (inst.fixed, self.out.get_mut(first_idx)) {
                first.fixed_start = Some(f);
            }
            items_done = 0;
            prev_block = Some(inst);
        }
    }

    pub(super) fn plan_sequential(&mut self, playlist: &str, block: &'a Block, req: &PlanRequest) {
        let (from, until) = (req.from, req.until);
        if self.playlist(playlist).is_none() {
            self.warn(WarningKind::Config, Some(from), format!("sequential playlist '{playlist}' not found"));
            self.slate(from, diff_ms(until, from), None);
            return;
        }
        let inst = Inst { key: "sequential:main".into(), block, fixed: None };
        let t = self.fill_block(&inst, from, None, false, req.block_items_done, until);
        if t < until && self.out.is_empty() {
            self.slate(t, diff_ms(until, t), None);
        }
    }

    /// Rules with the `between_blocks` trigger.
    pub(super) fn between_blocks(&mut self, prev: &Inst<'a>, t: &mut WallTime) {
        let ctx = self.block_ctx(prev.block, *t);
        for r in self.firing_rules(|tr| matches!(tr, Trigger::BetweenBlocks), &ctx) {
            self.apply_rule(r, t, Some(prev));
        }
    }
}

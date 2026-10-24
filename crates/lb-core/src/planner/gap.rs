//! Gap filling with promos and fillers (best fit).

use crate::history::PlayHistory;
use crate::media::{MediaItem, MediaKind};
use crate::rundown::Origin;
use crate::{Ms, WallTime, diff_ms};

use super::context::{Inst, Planner};

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// Fill `[t, end)` with promos and fillers. Returns the new end
    /// (the remainder, if any, is left for the slate).
    pub(super) fn fill_gap(&mut self, mut t: WallTime, end: WallTime, prev: Option<&Inst<'a>>) -> WallTime {
        let min_fill = self.inp.rules.filler.min_fill.ms();
        let mut last: Option<String> = None;
        loop {
            let rem = diff_ms(end, t);
            if rem < min_fill {
                break;
            }
            let mut cands = self.gap_candidates(t, rem);
            // avoid the same item twice in a row when there is a choice
            if cands.len() > 1 {
                cands.retain(|m| Some(&m.id) != last.as_ref());
            }
            let Some(m) = self.best_fit(&cands) else { break };
            last = Some(m.id.clone());
            let mut it = self.item_for(m, Origin::Filler, t);
            it.optional = true;
            it.priority = -100;
            self.tag_block(&mut it, prev);
            self.hist.planned.record(&m.id, t);
            t = it.end();
            self.out.push(it);
        }
        t
    }

    /// A filler scene can be used (listed, enabled, a scene).
    pub(super) fn has_filler_scene(&self) -> bool {
        self.inp.rules.filler.scenes.iter().filter_map(|i| self.media(i)).any(|m| m.enabled && m.is_scene())
    }

    /// Cover `[t, end)` with one of the filler scenes stretched to it (the
    /// policy's `scenes`). Returns the new end.
    pub(super) fn scene_gap(&mut self, t: WallTime, end: WallTime, prev: Option<&Inst<'a>>) -> WallTime {
        let pol = &self.inp.rules.filler;
        let rem = diff_ms(end, t);
        if pol.scenes.is_empty() || rem < pol.min_fill.ms() {
            return t;
        }
        let cands: Vec<&'a MediaItem> = pol.scenes.iter().filter_map(|i| self.media(i)).filter(|m| m.enabled && m.is_scene()).collect();
        let Some(m) = self.select("filler-scene", &cands, crate::rules::SelectMode::Sequential, false) else { return t };
        let mut it = self.item_for(m, Origin::Filler, t);
        it.duration_ms = rem;
        it.optional = true;
        it.priority = -100;
        self.tag_block(&mut it, prev);
        self.hist.planned.record(&m.id, t);
        self.out.push(it);
        end
    }

    /// Promos (if enabled) and fillers fitting into `rem` ms.
    fn gap_candidates(&self, t: WallTime, rem: Ms) -> Vec<&'a MediaItem> {
        let pol = &self.inp.rules.filler;
        let mut cands: Vec<&'a MediaItem> = Vec::new();
        if pol.use_promos {
            cands.extend(self.relevant_promos(t, None, Some(rem)));
        }
        let cat_ok = |m: &MediaItem| {
            pol.categories.is_empty() || m.category.as_deref().is_some_and(|c| pol.categories.iter().any(|x| x.eq_ignore_ascii_case(c)))
        };
        cands.extend(
            self.inp
                .library
                .by_kind_category(Some(MediaKind::Filler), None)
                .filter(|m| cat_ok(m) && m.play_duration() > 0 && m.play_duration() <= rem)
                // gap scenes cover the rest, stretched; not as fixed-length fillers
                .filter(|m| !pol.scenes.contains(&m.id))
                .filter(|m| self.hist.last_played(&m.id).is_none_or(|lp| diff_ms(t, lp) >= pol.min_interval.ms())),
        );
        cands
    }

    /// Longest candidate; ties broken randomly (deterministic seed).
    fn best_fit(&mut self, cands: &[&'a MediaItem]) -> Option<&'a MediaItem> {
        let best = cands.iter().map(|m| m.play_duration()).max()?;
        let top: Vec<&'a MediaItem> = cands.iter().copied().filter(|m| m.play_duration() == best).collect();
        Some(top[self.rng.below(top.len())])
    }
}

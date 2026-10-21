//! Promo relevance: promote only programmes that start later within the
//! lookahead window; promos expire once their programme has started.

use crate::history::PlayHistory;
use crate::media::{MediaItem, MediaKind, PromoTarget};
use crate::schedule::BlockSource;
use crate::{Ms, WallTime, add_ms, diff_ms};

use super::context::{Inst, Planner};

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// Promos eligible at `t` (optionally of a category / not longer than `max_ms`).
    pub(super) fn relevant_promos(&self, t: WallTime, category: Option<&str>, max_ms: Option<Ms>) -> Vec<&'a MediaItem> {
        let pol = &self.inp.rules.promos;
        let horizon = add_ms(t, pol.lookahead.ms());
        let upcoming: Vec<Inst<'a>> =
            self.instances(t, horizon).into_iter().filter(|i| i.fixed.is_some_and(|f| f > t && f <= horizon)).collect();
        self.inp
            .library
            .by_kind_category(Some(MediaKind::Promo), category)
            .filter(|m| m.play_duration() > 0 && max_ms.is_none_or(|mx| m.play_duration() <= mx))
            .filter(|m| self.hist.last_played(&m.id).is_none_or(|lp| diff_ms(t, lp) >= pol.min_interval.ms()))
            .filter(|m| match &m.promo {
                None => true, // generic channel promo
                Some(target) => upcoming.iter().any(|i| targets(target, i)),
            })
            .collect()
    }
}

/// Whether the promo target refers to this block instance.
fn targets(target: &PromoTarget, i: &Inst) -> bool {
    target.block.as_deref().is_some_and(|b| b == i.block.id)
        || target.category.as_deref().is_some_and(|c| i.block.category.as_deref().is_some_and(|bc| bc.eq_ignore_ascii_case(c)))
        || target
            .media
            .as_deref()
            .is_some_and(|mid| matches!(&i.block.source, BlockSource::Media { items } if items.iter().any(|x| x == mid)))
}

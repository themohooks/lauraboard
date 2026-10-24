//! Interstitial rule evaluation and media selection.

use crate::WallTime;
use crate::media::{MediaItem, MediaKind};
use crate::rules::{InsertAction, InsertionRule, SelectMode};
use crate::rundown::{Origin, WarningKind};

use super::context::{Inst, MatchCtxOwned, Planner};

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// Enabled rules with a matching trigger and conditions, ordered by
    /// priority; only the highest of each group survives.
    pub(super) fn firing_rules(&self, trig: impl Fn(&InsertionRule) -> bool, ctx: &MatchCtxOwned) -> Vec<&'a InsertionRule> {
        let mctx = ctx.as_ctx();
        let mut v: Vec<&'a InsertionRule> =
            self.inp.rules.rules.iter().filter(|r| r.enabled && trig(r) && r.conditions.matches(&mctx)).collect();
        v.sort_by_key(|r| std::cmp::Reverse(r.priority));
        let mut seen_groups: Vec<&str> = Vec::new();
        v.retain(|r| match &r.group {
            Some(g) if seen_groups.contains(&g.as_str()) => false,
            Some(g) => {
                seen_groups.push(g);
                true
            }
            None => true,
        });
        v
    }

    /// Insert the rule's media at `t` (advancing `t`).
    pub(super) fn apply_rule(&mut self, r: &'a InsertionRule, t: &mut WallTime, inst: Option<&Inst<'a>>) {
        if r.probability < 1.0 && self.rng.next_f64() >= r.probability {
            return;
        }
        let before = self.out.len();
        match &r.action {
            InsertAction::Jingle { category, items } => {
                let cands = if items.is_empty() {
                    self.inp
                        .library
                        .by_kind_category(Some(MediaKind::Jingle), category.as_deref())
                        .filter(|m| m.play_duration() > 0)
                        .collect()
                } else {
                    self.media_list(items)
                };
                if let Some(m) = self.select(&r.id, &cands, r.select, r.no_repeat) {
                    self.push_interstitial(m, Origin::Rule, t, inst, Some(r));
                }
            }
            InsertAction::Media { items } => {
                let cands = self.media_list(items);
                if let Some(m) = self.select(&r.id, &cands, r.select, r.no_repeat) {
                    self.push_interstitial(m, Origin::Rule, t, inst, Some(r));
                }
            }
            InsertAction::Promos { count, category } => {
                let mut used: Vec<String> = Vec::new();
                for _ in 0..*count {
                    let cands: Vec<_> =
                        self.relevant_promos(*t, category.as_deref(), None).into_iter().filter(|m| !used.contains(&m.id)).collect();
                    let Some(m) = self.select(&r.id, &cands, r.select, r.no_repeat) else { break };
                    used.push(m.id.clone());
                    self.push_interstitial(m, Origin::Rule, t, inst, Some(r));
                }
            }
            InsertAction::Scene { items, duration } => {
                let cands: Vec<&'a MediaItem> = if items.is_empty() {
                    self.inp.library.items.iter().filter(|m| m.enabled && m.is_scene()).collect()
                } else {
                    items.iter().filter_map(|i| self.media(i)).filter(|m| m.enabled && m.is_scene()).collect()
                };
                let len = duration.map(|d| d.ms()).filter(|d| *d > 0);
                let cands: Vec<&'a MediaItem> = cands.into_iter().filter(|m| len.is_some() || m.play_duration() > 0).collect();
                if let Some(m) = self.select(&r.id, &cands, r.select, r.no_repeat) {
                    self.push_interstitial(m, Origin::Rule, t, inst, Some(r));
                    if let (Some(ms), Some(it)) = (len, self.out.last_mut()) {
                        it.duration_ms = ms;
                        *t = it.end();
                    }
                }
            }
            InsertAction::AdBreak { ad_break } => {
                let Some(brk) = self.inp.rules.ad_break(ad_break) else {
                    self.warn(WarningKind::Config, Some(*t), format!("rule '{}' references unknown ad break '{ad_break}'", r.id));
                    return;
                };
                let items = self.build_ad_break(brk, *t, Some(&r.id), r.optional, r.priority);
                *t = self.push_ad_items(items, *t, inst, r.skip_on_manual);
            }
        }
        if self.out.len() > before {
            self.st.rule_last_fired.insert(r.id.clone(), self.out[before].start);
        }
    }

    fn media_list(&self, ids: &[String]) -> Vec<&'a MediaItem> {
        ids.iter().filter_map(|i| self.media(i)).filter(|m| m.enabled && m.play_duration() > 0).collect()
    }

    /// Pick one candidate according to the selection mode; remembers the
    /// pick under `key` for rotation / no-repeat.
    pub(super) fn select(&mut self, key: &str, cands: &[&'a MediaItem], mode: SelectMode, no_repeat: bool) -> Option<&'a MediaItem> {
        if cands.is_empty() {
            return None;
        }
        let last = self.st.last_pick.get(key).cloned();
        let pick = match mode {
            SelectMode::Sequential => {
                let pos = last.as_ref().and_then(|l| cands.iter().position(|m| &m.id == l));
                cands[pos.map(|p| (p + 1) % cands.len()).unwrap_or(0)]
            }
            SelectMode::Random => {
                let avoid = if no_repeat { last.as_deref() } else { None };
                self.pick_weighted(cands, avoid)?
            }
        };
        self.st.last_pick.insert(key.to_string(), pick.id.clone());
        Some(pick)
    }
}

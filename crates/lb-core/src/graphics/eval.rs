//! Rule evaluation: which elements are visible right now.

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::config::{GraphicRule, GraphicsConfig};
use crate::rules::MatchCtx;
use crate::rundown::RundownItem;
use crate::{Ms, WallTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Manual {
    Show,
    Hide,
}

pub struct GfxContext<'a> {
    pub now: WallTime,
    pub item: Option<&'a RundownItem>,
    /// Elapsed / remaining time of the current item.
    pub elapsed_ms: Ms,
    pub remaining_ms: Ms,
    pub profile: Option<&'a str>,
    pub manual: &'a HashMap<String, Manual>,
    /// Template variables, for rules that hide empty text (`None`: such
    /// rules are not checked).
    pub vars: Option<&'a HashMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveGraphic {
    pub element: String,
    pub layer: i32,
    pub rule: Option<String>,
    pub priority: i32,
    /// Time since it became visible (for in-animation).
    pub shown_ms: Ms,
    /// Time until it hides, if known (for out-animation).
    pub hide_in_ms: Option<Ms>,
}

fn profile_ok(list: &[String], p: Option<&str>) -> bool {
    list.is_empty() || p.is_some_and(|p| list.iter().any(|x| x == p))
}

/// Visibility window for a rule; returns (shown_ms, hide_in_ms).
fn rule_window(r: &GraphicRule, ctx: &GfxContext) -> Option<(Ms, Option<Ms>)> {
    let tm = &r.timing;
    if !tm.at.is_empty() {
        let day_ms = |t: NaiveTime| (chrono::Timelike::num_seconds_from_midnight(&t) as Ms) * 1000;
        let now = day_ms(ctx.now.time()) + chrono::Timelike::nanosecond(&ctx.now.time()) as Ms / 1_000_000;
        let dur = tm.duration.map(|d| d.ms()).unwrap_or(60_000);
        for at in &tm.at {
            let mut since = now - day_ms(*at);
            if since < 0 {
                since += 86_400_000;
            }
            if since < dur {
                return Some((since, Some(dur - since)));
            }
        }
        return None;
    }
    let item_bound =
        tm.after_start.is_some() || tm.before_end.is_some() || tm.every.is_some() || tm.duration.is_some() || tm.last.is_some();
    if item_bound && ctx.item.is_none() {
        return None;
    }
    if let Some(last) = tm.last.map(|d| d.ms()) {
        let hide_at = tm.before_end.map(|d| d.ms()).unwrap_or(0);
        if ctx.remaining_ms > last || ctx.remaining_ms <= hide_at {
            return None;
        }
        return Some((last - ctx.remaining_ms, Some(ctx.remaining_ms - hide_at)));
    }
    let start = tm.after_start.map(|d| d.ms()).unwrap_or(0);
    let mut since = ctx.elapsed_ms - start;
    if since < 0 {
        return None;
    }
    if let Some(every) = tm.every.map(|d| d.ms()).filter(|e| *e > 0) {
        since %= every;
    }
    let mut hide_in: Option<Ms> = None;
    if let Some(d) = tm.duration.map(|d| d.ms()) {
        if since >= d {
            return None;
        }
        hide_in = Some(d - since);
    }
    if let Some(be) = tm.before_end.map(|d| d.ms()) {
        let left = ctx.remaining_ms - be;
        if left <= 0 {
            return None;
        }
        hide_in = Some(hide_in.map_or(left, |h| h.min(left)));
    }
    Some((since, hide_in))
}

/// All text the rule would show for `el` renders empty.
fn text_is_empty(r: &GraphicRule, el: &super::model::Element, vars: &HashMap<String, String>) -> bool {
    let applied = r.apply_texts(el);
    let el = applied.as_ref().unwrap_or(el);
    super::config::element_texts(el).is_some_and(|t| t.iter().all(|t| super::render_template(t, vars).trim().is_empty()))
}

/// Compute visible elements, ordered by layer then priority.
pub fn evaluate(cfg: &GraphicsConfig, ctx: &GfxContext) -> Vec<ActiveGraphic> {
    let mctx = MatchCtx {
        at: Some(ctx.now),
        category: ctx.item.and_then(|i| i.category.as_deref()),
        playlist: ctx.item.and_then(|i| i.playlist.as_deref()),
        block: ctx.item.and_then(|i| i.block.as_deref()),
        media: ctx.item.map(|i| i.media.as_str()),
        kind: ctx.item.map(|i| i.kind),
    };
    let mut out: Vec<ActiveGraphic> = Vec::new();
    for r in cfg.rules.iter().filter(|r| r.enabled && profile_ok(&r.profiles, ctx.profile)) {
        let Some(el) = cfg.element(&r.element) else { continue };
        if !profile_ok(&el.profiles, ctx.profile) || ctx.manual.get(&el.id) == Some(&Manual::Hide) {
            continue;
        }
        if !r.conditions.matches(&mctx) {
            continue;
        }
        let Some((shown, hide_in)) = rule_window(r, ctx) else { continue };
        if r.hide_if_empty && ctx.vars.is_some_and(|vars| text_is_empty(r, el, vars)) {
            continue;
        }
        let layer = r.layer.unwrap_or(el.layer);
        if let Some(existing) = out.iter_mut().find(|a| a.element == el.id) {
            if r.priority > existing.priority {
                *existing = ActiveGraphic {
                    element: el.id.clone(),
                    layer,
                    rule: Some(r.id.clone()),
                    priority: r.priority,
                    shown_ms: shown,
                    hide_in_ms: hide_in,
                };
            }
            continue;
        }
        out.push(ActiveGraphic {
            element: el.id.clone(),
            layer,
            rule: Some(r.id.clone()),
            priority: r.priority,
            shown_ms: shown,
            hide_in_ms: hide_in,
        });
    }
    // manual show
    for (id, m) in ctx.manual {
        if *m == Manual::Show
            && !out.iter().any(|a| &a.element == id)
            && let Some(el) = cfg.element(id)
        {
            out.push(ActiveGraphic {
                element: id.clone(),
                layer: el.layer,
                rule: None,
                priority: i32::MAX,
                shown_ms: 10_000,
                hide_in_ms: None,
            });
        }
    }
    // exclusive layers
    out.sort_by(|a, b| a.layer.cmp(&b.layer).then(b.priority.cmp(&a.priority)).then(a.element.cmp(&b.element)));
    let mut seen_exclusive: Vec<i32> = Vec::new();
    out.retain(|a| {
        if !cfg.layer_exclusive(a.layer) {
            return true;
        }
        if seen_exclusive.contains(&a.layer) {
            return false;
        }
        seen_exclusive.push(a.layer);
        true
    });
    out
}

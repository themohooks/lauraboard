//! Automation rules (idents / ads / promos), ad breaks, policies and
//! transitions: draft with undo, form validation, wizard, plan check,
//! firing journal.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use chrono::{Local, NaiveTime};
use serde_json::json;
use slint::{ComponentHandle, SharedString};

use lb_core::Dur;
use lb_core::history::AsRunRecord;
use lb_core::rules::{AdBreak, AdOrder, Conditions, InsertAction, InsertionRule, RuleSet, SelectMode, Trigger};
use lb_core::rundown::Rundown;

use super::{Ctx, vec_model, with};
use crate::fmt::KINDS;
use crate::logic::filter;
use crate::logic::rule_check::{self, action_kind, action_text, is_ad_rule, trigger_text};
use crate::logic::undo::History;
use crate::net::Msg;
use crate::shell::{AppWindow, RuleRow, Rules};

#[derive(Default)]
pub struct RulesState {
    saved: Option<RuleSet>,
    draft: Option<RuleSet>,
    history: History<RuleSet>,
    scope: String,
    selected: Option<usize>,
    wizard: Option<InsertionRule>,
    brk: Option<usize>,
    last_edit: Option<(String, Instant)>,
}

impl RulesState {
    pub fn dirty(&self) -> bool {
        self.draft.is_some() && self.draft != self.saved
    }
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Rules>();
    g.on_select(|i| {
        with(move |c| {
            // -1: back to the list (narrow window)
            c.rules.selected = usize::try_from(i).ok();
            refresh(c);
        })
    });
    g.on_toggle_enabled(|i| {
        with(move |c| {
            mutate(c, None, |set| {
                if let Some(r) = set.rules.get_mut(i as usize) {
                    r.enabled = !r.enabled;
                }
            });
            refresh(c);
        })
    });
    g.on_filter(|| with(refresh));
    g.on_form_changed(|| with(form_changed));
    g.on_toggle_kind(|i| with(move |c| toggle_list(c, true, i as usize)));
    g.on_toggle_weekday(|i| with(move |c| toggle_list(c, false, i as usize)));
    g.on_new_rule(|| with(new_rule));
    g.on_wizard_finish(|| with(wizard_finish));
    g.on_wizard_cancel(|| {
        with(|c| {
            c.rules.wizard = None;
            if let Some(ui) = c.window() {
                ui.global::<Rules>().set_wizard_open(false);
            }
            refresh(c);
        })
    });
    g.on_duplicate(|| with(duplicate));
    g.on_remove(|| with(remove));
    g.on_test_rule(|| with(test_rule));
    g.on_load_journal(|| with(load_journal));
    g.on_break_select(|i| {
        with(move |c| {
            c.rules.brk = Some(i as usize);
            refresh(c);
        })
    });
    g.on_break_changed(|| with(break_changed));
    g.on_break_add(|| with(break_add));
    g.on_break_remove(|| with(break_remove));
    g.on_policy_changed(|| with(policy_changed));
    g.on_transitions_changed(|| with(transitions_changed));
    g.on_toggle_t_kind(|i| {
        with(move |c| {
            let Some(k) = KINDS.get(i as usize).copied() else { return };
            mutate(c, None, move |set| {
                let v = &mut set.transitions.kinds;
                if let Some(p) = v.iter().position(|x| *x == k) {
                    v.remove(p);
                } else {
                    v.push(k);
                }
            });
            refresh(c);
        })
    });
    g.on_save(|| with(save));
    g.on_revert(|| {
        with(|c| {
            let saved = c.rules.saved.clone();
            mutate(c, None, move |set| {
                if let Some(s) = saved {
                    *set = s;
                }
            });
            refresh(c);
        })
    });
    g.on_undo(|| with(undo));
    g.on_redo(|| with(redo));
}

/// Fresh rules from the server.
pub fn reloaded(c: &mut Ctx) {
    let was_dirty = c.rules.dirty();
    c.rules.saved = Some(c.model.rules.clone());
    if !was_dirty {
        c.rules.draft = c.rules.saved.clone();
    }
    if matches!(c.section().as_str(), "jingles" | "ads") {
        refresh(c);
    }
}

/// Select the first rule of the current scope (review / tooling).
#[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
pub fn select_first(c: &mut Ctx) {
    let set = c.rules.draft.clone().unwrap_or_default();
    c.rules.selected = set.rules.iter().position(|r| in_scope(c, r));
    refresh(c);
}

/// Screen opened with a scope ("jingles" / "ads").
pub fn opened(c: &mut Ctx, scope: &str) {
    if c.rules.draft.is_none() {
        c.rules.saved = Some(c.model.rules.clone());
        c.rules.draft = c.rules.saved.clone();
    }
    if c.rules.scope != scope {
        c.rules.scope = scope.to_string();
        c.rules.selected = None;
        if let Some(ui) = c.window() {
            let g = ui.global::<Rules>();
            g.set_tab(0);
            g.set_examples(vec_model(Vec::new()));
            g.set_journal(vec_model(Vec::new()));
            g.set_examples_note(SharedString::new());
        }
    }
    if let Some(ui) = c.window() {
        ui.global::<Rules>().set_scope(scope.into());
    }
    if c.rules.brk.is_none() && c.rules.draft.as_ref().is_some_and(|d| !d.ad_breaks.is_empty()) {
        c.rules.brk = Some(0);
    }
    refresh(c);
}

fn mutate(c: &mut Ctx, coalesce: Option<&str>, f: impl FnOnce(&mut RuleSet)) {
    let Some(before) = c.rules.draft.clone() else { return };
    let mut after = before.clone();
    f(&mut after);
    let merge = match (coalesce, &c.rules.last_edit) {
        (Some(k), Some((lk, at))) => k == lk && at.elapsed() < Duration::from_millis(1500),
        _ => false,
    };
    if !merge {
        c.rules.history.record(&before, &after);
    }
    c.rules.last_edit = coalesce.map(|k| (k.to_string(), Instant::now()));
    c.rules.draft = Some(after);
}

fn in_scope(c: &Ctx, r: &InsertionRule) -> bool {
    (c.rules.scope == "ads") == is_ad_rule(r)
}

fn categories(c: &Ctx) -> HashSet<String> {
    filter::categories(&c.model.library).into_iter().collect()
}

fn s(v: impl Into<SharedString>) -> SharedString {
    v.into()
}

fn bools(v: Vec<bool>) -> slint::ModelRc<bool> {
    vec_model(v)
}

/// Push the whole screen state (list, form, breaks, policies).
pub fn refresh(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let set = c.rules.draft.clone().unwrap_or_default();
    let cats = categories(c);
    let search = g.get_search().to_lowercase();
    let rows: Vec<RuleRow> = set
        .rules
        .iter()
        .enumerate()
        .filter(|(_, r)| in_scope(c, r))
        .filter(|(_, r)| search.is_empty() || r.name.to_lowercase().contains(&search) || r.id.to_lowercase().contains(&search))
        .map(|(i, r)| RuleRow {
            index: i as i32,
            id: s(r.id.clone()),
            name: s(if r.name.is_empty() { r.id.clone() } else { r.name.clone() }),
            summary: s(format!("{} → {}", trigger_text(&r.trigger), action_text(&r.action))),
            kind: s(action_kind(&r.action)),
            enabled: r.enabled,
            priority: r.priority,
            group: s(r.group.clone().unwrap_or_default()),
            problem: s(rule_check::problems(&set, i, &cats).into_iter().next().unwrap_or_default()),
        })
        .collect();
    if c.rules.selected.is_some_and(|i| set.rules.get(i).is_none_or(|r| !in_scope(c, r))) {
        c.rules.selected = None;
    }
    g.set_rules(vec_model(rows));
    g.set_selected(c.rules.selected.map(|i| i as i32).unwrap_or(-1));
    g.set_ad_breaks(vec_model(set.ad_breaks.iter().map(|b| s(if b.name.is_empty() { b.id.clone() } else { format!("{} — {}", b.id, b.name) })).collect()));
    g.set_dirty(c.rules.dirty());
    g.set_can_undo(c.rules.history.can_undo());
    g.set_can_redo(c.rules.history.can_redo());
    if c.rules.wizard.is_none() {
        match c.rules.selected.and_then(|i| set.rules.get(i)).cloned() {
            Some(r) => {
                g.set_has_form(true);
                load_form(c, &r);
                analysis(c);
            }
            None => g.set_has_form(false),
        }
    }
    breaks(c);
    policies(c);
}

fn join(v: &[String]) -> String {
    v.join(", ")
}

fn split(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

fn load_form(c: &Ctx, r: &InsertionRule) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let set = c.rules.draft.clone().unwrap_or_default();
    g.set_f_id(s(r.id.clone()));
    g.set_f_name(s(r.name.clone()));
    g.set_f_enabled(r.enabled);
    let (t, n, m, cat) = match &r.trigger {
        Trigger::EveryNItems { n } => (0, n.to_string(), "30".to_string(), String::new()),
        Trigger::EveryMinutes { minutes } => (1, "3".into(), minutes.to_string(), String::new()),
        Trigger::BlockStart => (2, "3".into(), "30".into(), String::new()),
        Trigger::BlockEnd => (3, "3".into(), "30".into(), String::new()),
        Trigger::BetweenBlocks => (4, "3".into(), "30".into(), String::new()),
        Trigger::AfterCategory { category } => (5, "3".into(), "30".into(), category.clone()),
        Trigger::BeforeCategory { category } => (6, "3".into(), "30".into(), category.clone()),
    };
    g.set_f_trigger(t);
    g.set_f_n(s(n));
    g.set_f_minutes(s(m));
    g.set_f_tcat(s(cat));
    let (a, jcat, items, ab, pcount, pcat) = match &r.action {
        InsertAction::Jingle { category, items } => (0, category.clone().unwrap_or_default(), join(items), -1, "1".to_string(), String::new()),
        InsertAction::AdBreak { ad_break } => {
            (1, String::new(), String::new(), set.ad_breaks.iter().position(|b| &b.id == ad_break).map(|i| i as i32).unwrap_or(-1), "1".into(), String::new())
        }
        InsertAction::Promos { count, category } => (2, String::new(), String::new(), -1, count.to_string(), category.clone().unwrap_or_default()),
        InsertAction::Media { items } => (3, String::new(), join(items), -1, "1".into(), String::new()),
    };
    g.set_f_action(a);
    g.set_f_jcat(s(jcat));
    g.set_f_items(s(items));
    g.set_f_ab(ab);
    g.set_f_pcount(s(pcount));
    g.set_f_pcat(s(pcat));
    let cd = &r.conditions;
    g.set_f_cats(s(join(&cd.categories)));
    g.set_f_xcats(s(join(&cd.exclude_categories)));
    g.set_f_blocks(s(join(&cd.blocks)));
    g.set_f_playlists(s(join(&cd.playlists)));
    g.set_f_xmedia(s(join(&cd.exclude_media)));
    g.set_f_kinds(bools(KINDS.iter().map(|k| cd.kinds.contains(k)).collect()));
    g.set_f_weekdays(bools((1..=7).map(|d| cd.weekdays.contains(&d)).collect()));
    g.set_f_from(s(cd.from.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()));
    g.set_f_to(s(cd.to.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()));
    g.set_f_random(r.select == SelectMode::Random);
    g.set_f_norepeat(r.no_repeat);
    g.set_f_prob(r.probability as f32);
    g.set_f_priority(s(r.priority.to_string()));
    g.set_f_group(s(r.group.clone().unwrap_or_default()));
    g.set_f_optional(r.optional);
    g.set_f_skipmanual(r.skip_on_manual);
    for f in [Rules::set_e_n, Rules::set_e_minutes, Rules::set_e_from, Rules::set_e_to, Rules::set_e_priority, Rules::set_e_pcount] {
        f(&g, SharedString::new());
    }
}

/// Read the form over `base`; invalid fields keep the old value and show an error.
fn read_form(c: &Ctx, base: &InsertionRule) -> InsertionRule {
    let Some(ui) = c.window() else { return base.clone() };
    let g = ui.global::<Rules>();
    let set = c.rules.draft.clone().unwrap_or_default();
    let mut r = base.clone();
    r.name = g.get_f_name().trim().to_string();
    r.enabled = g.get_f_enabled();
    let n: Option<usize> = g.get_f_n().trim().parse().ok().filter(|n| *n > 0);
    let m: Option<f64> = g.get_f_minutes().trim().replace(',', ".").parse().ok().filter(|m: &f64| *m > 0.0 && *m <= 1440.0);
    g.set_e_n(s(if g.get_f_trigger() == 0 && n.is_none() { "Целое число больше 0" } else { "" }));
    g.set_e_minutes(s(if g.get_f_trigger() == 1 && m.is_none() { "От 1 до 1440" } else { "" }));
    let old_n = if let Trigger::EveryNItems { n } = r.trigger { n } else { 3 };
    let old_m = if let Trigger::EveryMinutes { minutes } = r.trigger { minutes } else { 30.0 };
    let tcat = g.get_f_tcat().trim().to_string();
    r.trigger = match g.get_f_trigger() {
        0 => Trigger::EveryNItems { n: n.unwrap_or(old_n) },
        1 => Trigger::EveryMinutes { minutes: m.unwrap_or(old_m) },
        2 => Trigger::BlockStart,
        3 => Trigger::BlockEnd,
        4 => Trigger::BetweenBlocks,
        5 => Trigger::AfterCategory { category: tcat },
        _ => Trigger::BeforeCategory { category: tcat },
    };
    let pc: Option<usize> = g.get_f_pcount().trim().parse().ok().filter(|n| (1..=10).contains(n));
    g.set_e_pcount(s(if g.get_f_action() == 2 && pc.is_none() { "От 1 до 10" } else { "" }));
    let opt = |v: SharedString| Some(v.trim().to_string()).filter(|x| !x.is_empty());
    r.action = match g.get_f_action() {
        0 => InsertAction::Jingle { category: opt(g.get_f_jcat()), items: split(&g.get_f_items()) },
        1 => InsertAction::AdBreak {
            ad_break: set
                .ad_breaks
                .get(g.get_f_ab().max(0) as usize)
                .map(|b| b.id.clone())
                .or_else(|| if let InsertAction::AdBreak { ad_break } = &base.action { Some(ad_break.clone()) } else { None })
                .unwrap_or_default(),
        },
        2 => InsertAction::Promos {
            count: pc.unwrap_or(if let InsertAction::Promos { count, .. } = base.action { count } else { 1 }),
            category: opt(g.get_f_pcat()),
        },
        _ => InsertAction::Media { items: split(&g.get_f_items()) },
    };
    let time = |v: SharedString, is_from: bool, old: Option<NaiveTime>| -> Option<NaiveTime> {
        let (t, err) = match crate::logic::timeline::parse_time(&v) {
            Ok(t) => (t, ""),
            Err(e) => (old, e),
        };
        if is_from {
            g.set_e_from(s(err));
        } else {
            g.set_e_to(s(err));
        }
        t
    };
    let cd = Conditions {
        categories: split(&g.get_f_cats()),
        exclude_categories: split(&g.get_f_xcats()),
        playlists: split(&g.get_f_playlists()),
        blocks: split(&g.get_f_blocks()),
        exclude_media: split(&g.get_f_xmedia()),
        from: time(g.get_f_from(), true, base.conditions.from),
        to: time(g.get_f_to(), false, base.conditions.to),
        ..base.conditions.clone()
    };
    r.conditions = cd;
    r.select = if g.get_f_random() { SelectMode::Random } else { SelectMode::Sequential };
    r.no_repeat = g.get_f_norepeat();
    r.probability = (g.get_f_prob() as f64).clamp(0.0, 1.0);
    match g.get_f_priority().trim().parse::<i32>() {
        Ok(p) => {
            r.priority = p;
            g.set_e_priority(SharedString::new());
        }
        Err(_) => g.set_e_priority(s("Целое число")),
    }
    r.group = opt(g.get_f_group());
    r.optional = g.get_f_optional();
    r.skip_on_manual = g.get_f_skipmanual();
    r
}

fn form_changed(c: &mut Ctx) {
    if let Some(w) = c.rules.wizard.clone() {
        c.rules.wizard = Some(read_form(c, &w));
        analysis(c);
        return;
    }
    let Some(i) = c.rules.selected else { return };
    let Some(base) = c.rules.draft.as_ref().and_then(|d| d.rules.get(i)).cloned() else { return };
    let r = read_form(c, &base);
    mutate(c, Some("form"), move |set| set.rules[i] = r);
    list_and_flags(c);
    analysis(c);
}

/// Update the list and the save flags without touching the form.
fn list_and_flags(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let set = c.rules.draft.clone().unwrap_or_default();
    let cats = categories(c);
    let rows: Vec<RuleRow> = set
        .rules
        .iter()
        .enumerate()
        .filter(|(_, r)| in_scope(c, r))
        .map(|(i, r)| RuleRow {
            index: i as i32,
            id: s(r.id.clone()),
            name: s(if r.name.is_empty() { r.id.clone() } else { r.name.clone() }),
            summary: s(format!("{} → {}", trigger_text(&r.trigger), action_text(&r.action))),
            kind: s(action_kind(&r.action)),
            enabled: r.enabled,
            priority: r.priority,
            group: s(r.group.clone().unwrap_or_default()),
            problem: s(rule_check::problems(&set, i, &cats).into_iter().next().unwrap_or_default()),
        })
        .collect();
    g.set_rules(vec_model(rows));
    g.set_dirty(c.rules.dirty());
    g.set_can_undo(c.rules.history.can_undo());
    g.set_can_redo(c.rules.history.can_redo());
}

/// Problems and dependencies of the rule in the form.
fn analysis(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let mut set = c.rules.draft.clone().unwrap_or_default();
    let i = match (&c.rules.wizard, c.rules.selected) {
        (Some(w), _) => {
            set.rules.push(w.clone());
            set.rules.len() - 1
        }
        (None, Some(i)) => i,
        _ => return,
    };
    let cats = categories(c);
    g.set_problems(vec_model(rule_check::problems(&set, i, &cats).into_iter().map(SharedString::from).collect()));
    g.set_deps(vec_model(rule_check::dependencies(&set, i).into_iter().map(SharedString::from).collect()));
}

fn toggle_list(c: &mut Ctx, kinds: bool, i: usize) {
    let edit = move |r: &mut InsertionRule| {
        if kinds {
            if let Some(k) = KINDS.get(i).copied() {
                let v = &mut r.conditions.kinds;
                if let Some(p) = v.iter().position(|x| *x == k) {
                    v.remove(p);
                } else {
                    v.push(k);
                }
            }
        } else {
            let d = i as u32 + 1;
            let v = &mut r.conditions.weekdays;
            if let Some(p) = v.iter().position(|x| *x == d) {
                v.remove(p);
            } else {
                v.push(d);
                v.sort();
            }
        }
    };
    if let Some(w) = c.rules.wizard.as_mut() {
        edit(w);
        let w = w.clone();
        load_form(c, &w);
        return;
    }
    let Some(idx) = c.rules.selected else { return };
    mutate(c, None, |set| {
        if let Some(r) = set.rules.get_mut(idx) {
            edit(r);
        }
    });
    refresh(c);
}

fn blank_rule(c: &Ctx) -> InsertionRule {
    let set = c.rules.draft.clone().unwrap_or_default();
    let ads = c.rules.scope == "ads";
    InsertionRule {
        id: rule_check::new_id(&set, if ads { "ads" } else { "ident" }),
        name: if ads { "Реклама".into() } else { "Заставка".into() },
        enabled: true,
        trigger: if ads { Trigger::EveryMinutes { minutes: 30.0 } } else { Trigger::BlockStart },
        action: if ads {
            InsertAction::AdBreak { ad_break: set.ad_breaks.first().map(|b| b.id.clone()).unwrap_or_default() }
        } else {
            InsertAction::Jingle { category: Some("idents".into()), items: vec![] }
        },
        conditions: Conditions::default(),
        select: SelectMode::Random,
        probability: 1.0,
        no_repeat: true,
        priority: 0,
        group: None,
        optional: !ads,
        skip_on_manual: true,
    }
}

pub fn new_rule(c: &mut Ctx) {
    let r = blank_rule(c);
    load_form(c, &r);
    c.rules.wizard = Some(r);
    analysis(c);
    if let Some(ui) = c.window() {
        let g = ui.global::<Rules>();
        g.set_wizard_step(0);
        g.set_wizard_open(true);
    }
}

fn wizard_finish(c: &mut Ctx) {
    let Some(w) = c.rules.wizard.clone() else { return };
    let r = read_form(c, &w);
    let name = if r.name.is_empty() { r.id.clone() } else { r.name.clone() };
    mutate(c, None, |set| set.rules.push(r));
    c.rules.wizard = None;
    c.rules.selected = c.rules.draft.as_ref().map(|d| d.rules.len() - 1);
    if let Some(ui) = c.window() {
        ui.global::<Rules>().set_wizard_open(false);
    }
    refresh(c);
    c.toast(format!("Правило «{name}» добавлено — не забудьте сохранить"), false);
}

fn duplicate(c: &mut Ctx) {
    let Some(i) = c.rules.selected else { return };
    let Some(mut r) = c.rules.draft.as_ref().and_then(|d| d.rules.get(i)).cloned() else { return };
    let set = c.rules.draft.clone().unwrap_or_default();
    r.id = rule_check::new_id(&set, &r.id);
    r.name = format!("{} (копия)", r.name);
    r.enabled = false;
    mutate(c, None, |set| set.rules.insert(i + 1, r));
    c.rules.selected = Some(i + 1);
    refresh(c);
}

fn remove(c: &mut Ctx) {
    let Some(i) = c.rules.selected else { return };
    let name = c.rules.draft.as_ref().and_then(|d| d.rules.get(i)).map(|r| r.name.clone()).unwrap_or_default();
    mutate(c, None, |set| {
        set.rules.remove(i);
    });
    c.rules.selected = None;
    refresh(c);
    c.toast_with(format!("Правило «{name}» удалено"), "Отменить", undo);
}

fn undo(c: &mut Ctx) {
    let Some(cur) = c.rules.draft.clone() else { return };
    if let Some(prev) = c.rules.history.undo(&cur) {
        c.rules.draft = Some(prev);
        c.rules.last_edit = None;
        refresh(c);
    }
}

fn redo(c: &mut Ctx) {
    let Some(cur) = c.rules.draft.clone() else { return };
    if let Some(next) = c.rules.history.redo(&cur) {
        c.rules.draft = Some(next);
        c.rules.last_edit = None;
        refresh(c);
    }
}

fn save(c: &mut Ctx) {
    if !c.rules.dirty() {
        return;
    }
    if let Some(set) = c.rules.draft.clone() {
        c.save_doc("/rules", set, "Правила");
    }
}

pub fn shortcut(c: &mut Ctx, name: &str) {
    match name {
        "save" => save(c),
        "undo" => undo(c),
        "redo" => redo(c),
        _ => {}
    }
}

// ------------------------------------------------------------ check / journal

/// Plan 24 h with the draft rules and list where the rule fires.
fn test_rule(c: &mut Ctx) {
    let Some(i) = c.rules.selected else { return };
    let Some(rule_id) = c.rules.draft.as_ref().and_then(|d| d.rules.get(i)).map(|r| r.id.clone()) else { return };
    let Some(ui) = c.window() else { return };
    ui.global::<Rules>().set_checking(true);
    let ch = c.model.ch();
    let draft = c.rules.draft.clone().unwrap_or_default();
    let dirty = c.rules.dirty();
    let from = Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string();
    c.worker.spawn(move |api| {
        let mut note = String::new();
        let plan: Result<Rundown, String> = if dirty {
            match api.post(&format!("/channels/{ch}/plan"), &json!({ "from": from, "hours": 24, "rules": draft })) {
                Ok(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
                Err(_) => {
                    note = "Проверено по сохранённым правилам: сервер не поддерживает предпросмотр черновика. Сохраните правило, чтобы проверить изменения.".into();
                    api.get(&format!("/channels/{ch}/plan?hours=24"))
                }
            }
        } else {
            api.get(&format!("/channels/{ch}/plan?hours=24"))
        };
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Rules>();
            g.set_checking(false);
            match plan {
                Ok(p) => {
                    let mut lines = Vec::new();
                    let mut prev: Option<&str> = None;
                    for it in &p.items {
                        if it.rule.as_deref() == Some(rule_id.as_str()) && lines.len() < 12 {
                            lines.push(SharedString::from(format!(
                                "{} — «{}»{}",
                                it.start.format("%d.%m %H:%M:%S"),
                                it.title,
                                prev.map(|t| format!(" после «{t}»")).unwrap_or_default()
                            )));
                        }
                        prev = Some(it.title.as_str());
                    }
                    let total = p.items.iter().filter(|it| it.rule.as_deref() == Some(rule_id.as_str())).count();
                    if total == 0 {
                        note = format!(
                            "{}За ближайшие 24 часа правило не сработает. Проверьте условия, категории и что правило включено.",
                            if note.is_empty() { String::new() } else { format!("{note} ") }
                        );
                    } else if note.is_empty() {
                        note = format!("Сработает {total} раз за 24 часа. Первые срабатывания:");
                    }
                    g.set_examples(vec_model(lines));
                    g.set_examples_note(s(note));
                }
                Err(e) => c.toast(format!("Проверка правила: {e}"), true),
            }
        })))
    });
}

/// Today's as-run records produced by the rule.
fn load_journal(c: &mut Ctx) {
    let Some(i) = c.rules.selected else { return };
    let Some(rule_id) = c.rules.draft.as_ref().and_then(|d| d.rules.get(i)).map(|r| r.id.clone()) else { return };
    let ch = c.model.ch();
    let today = Local::now().date_naive();
    let week_ago = today - chrono::Duration::days(6);
    c.worker.spawn(move |api| {
        let recs: Vec<AsRunRecord> = api.get(&format!("/channels/{ch}/asrun?from={week_ago}&to={today}"))?;
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let mine: Vec<&AsRunRecord> = recs.iter().filter(|r| r.rule.as_deref() == Some(rule_id.as_str())).collect();
            let mut lines: Vec<SharedString> = mine
                .iter()
                .rev()
                .take(15)
                .map(|r| s(format!("{} — «{}» · {}", r.started.format("%d.%m %H:%M:%S"), r.title, format!("{:?}", r.status).to_lowercase())))
                .collect();
            if lines.is_empty() {
                lines.push(s("За последние 7 дней срабатываний не было"));
            } else {
                lines.insert(0, s(format!("Срабатываний за 7 дней: {}", mine.len())));
            }
            ui.global::<Rules>().set_journal(vec_model(lines));
        })))
    });
}

// ------------------------------------------------------------ ad breaks

fn breaks(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let set = c.rules.draft.clone().unwrap_or_default();
    g.set_breaks(vec_model(set.ad_breaks.iter().map(|b| s(if b.name.is_empty() { b.id.clone() } else { format!("{} ({})", b.name, b.id) })).collect()));
    if c.rules.brk.is_none() && !set.ad_breaks.is_empty() {
        c.rules.brk = Some(0);
    }
    let sel = c.rules.brk.filter(|&i| i < set.ad_breaks.len());
    g.set_break_selected(sel.map(|i| i as i32).unwrap_or(-1));
    let Some(b) = sel.map(|i| &set.ad_breaks[i]) else { return };
    g.set_b_id(s(b.id.clone()));
    g.set_b_name(s(b.name.clone()));
    g.set_b_items(s(join(&b.items)));
    g.set_b_category(s(b.category.clone().unwrap_or_default()));
    g.set_b_order(match b.order {
        AdOrder::Listed => 0,
        AdOrder::Random => 1,
        AdOrder::LeastPlayed => 2,
    });
    g.set_b_duration(s(b.duration.map(|d| d.to_string()).unwrap_or_default()));
    g.set_b_max(s(b.max_spots.map(|n| n.to_string()).unwrap_or_default()));
    g.set_b_intro(s(b.intro.clone().unwrap_or_default()));
    g.set_b_outro(s(b.outro.clone().unwrap_or_default()));
    g.set_b_fill(s(b.fill_category.clone().unwrap_or_default()));
    g.set_e_b_duration(SharedString::new());
    g.set_e_b_max(SharedString::new());
    let used: Vec<SharedString> = set
        .rules
        .iter()
        .filter(|r| matches!(&r.action, InsertAction::AdBreak { ad_break } if ad_break == &b.id))
        .map(|r| s(format!("правило «{}»", if r.name.is_empty() { &r.id } else { &r.name })))
        .chain(c.model.schedules.schedules.iter().flat_map(|sc| {
            sc.templates.iter().flat_map(move |t| {
                t.blocks.iter().filter(|bl| matches!(&bl.source, lb_core::schedule::BlockSource::AdBreak { ad_break } if ad_break == &b.id)).map(move |bl| {
                    s(format!("сетка «{}», шаблон «{}», блок «{}»", sc.id, t.id, if bl.title.is_empty() { &bl.id } else { &bl.title }))
                })
            })
        }))
        .collect();
    g.set_b_used_by(vec_model(used));
}

fn break_changed(c: &mut Ctx) {
    let Some(i) = c.rules.brk else { return };
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let opt = |v: SharedString| Some(v.trim().to_string()).filter(|x| !x.is_empty());
    let dur_txt = g.get_b_duration().trim().to_string();
    let dur: Option<Option<Dur>> = if dur_txt.is_empty() { Some(None) } else { dur_txt.parse().ok().map(Some) };
    g.set_e_b_duration(s(if dur.is_none() { "Формат ЧЧ:ММ:СС, например 00:03:00" } else { "" }));
    let max_txt = g.get_b_max().trim().to_string();
    let max: Option<Option<usize>> = if max_txt.is_empty() { Some(None) } else { max_txt.parse().ok().filter(|n| *n > 0).map(Some) };
    g.set_e_b_max(s(if max.is_none() { "Целое число больше 0" } else { "" }));
    let (name, items, category, order) = (g.get_b_name().trim().to_string(), split(&g.get_b_items()), opt(g.get_b_category()), g.get_b_order());
    let (intro, outro, fill) = (opt(g.get_b_intro()), opt(g.get_b_outro()), opt(g.get_b_fill()));
    mutate(c, Some("break"), move |set| {
        let Some(b) = set.ad_breaks.get_mut(i) else { return };
        b.name = name;
        b.items = items;
        b.category = category;
        b.order = match order {
            1 => AdOrder::Random,
            2 => AdOrder::LeastPlayed,
            _ => AdOrder::Listed,
        };
        if let Some(d) = dur {
            b.duration = d;
        }
        if let Some(m) = max {
            b.max_spots = m;
        }
        b.intro = intro;
        b.outro = outro;
        b.fill_category = fill;
    });
    list_and_flags(c);
    if let Some(ui) = c.window() {
        let set = c.rules.draft.clone().unwrap_or_default();
        ui.global::<Rules>().set_breaks(vec_model(
            set.ad_breaks.iter().map(|b| s(if b.name.is_empty() { b.id.clone() } else { format!("{} ({})", b.name, b.id) })).collect(),
        ));
    }
}

fn break_add(c: &mut Ctx) {
    let set = c.rules.draft.clone().unwrap_or_default();
    let mut n = set.ad_breaks.len() + 1;
    while set.ad_breaks.iter().any(|b| b.id == format!("break-{n}")) {
        n += 1;
    }
    let b: AdBreak = serde_json::from_value(json!({ "id": format!("break-{n}"), "name": format!("Рекламный блок {n}"), "category": "commercials" }))
        .unwrap_or_else(|_| set.ad_breaks.first().cloned().unwrap_or_else(|| serde_json::from_value(json!({ "id": format!("break-{n}") })).expect("ad break")));
    mutate(c, None, |set| set.ad_breaks.push(b));
    c.rules.brk = c.rules.draft.as_ref().map(|d| d.ad_breaks.len() - 1);
    refresh(c);
}

fn break_remove(c: &mut Ctx) {
    let Some(i) = c.rules.brk else { return };
    let Some(b) = c.rules.draft.as_ref().and_then(|d| d.ad_breaks.get(i)).cloned() else { return };
    let used = c.rules.draft.as_ref().is_some_and(|d| d.rules.iter().any(|r| matches!(&r.action, InsertAction::AdBreak { ad_break } if ad_break == &b.id)));
    let go = move |c: &mut Ctx| {
        mutate(c, None, |set| {
            set.ad_breaks.remove(i);
        });
        c.rules.brk = None;
        refresh(c);
        c.toast_with(format!("Рекламный блок «{}» удалён", b.id), "Отменить", undo);
    };
    if used {
        c.ask("Удалить рекламный блок?", "На блок ссылаются правила — они перестанут работать.", "Удалить", go);
    } else {
        go(c);
    }
}

// ------------------------------------------------------------ policies

fn policies(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let set = c.rules.draft.clone().unwrap_or_default();
    g.set_p_ads_max(s(set.ads.max_per_day.map(|n| n.to_string()).unwrap_or_default()));
    g.set_p_ads_interval(s(set.ads.min_interval.map(|d| d.to_string()).unwrap_or_default()));
    g.set_p_promo_lookahead(s(set.promos.lookahead.to_string()));
    g.set_p_promo_interval(s(set.promos.min_interval.to_string()));
    g.set_p_filler_cats(s(join(&set.filler.categories)));
    g.set_p_filler_promos(set.filler.use_promos);
    g.set_p_filler_min(s(set.filler.min_fill.to_string()));
    g.set_p_filler_interval(s(set.filler.min_interval.to_string()));
    g.set_t_fade(s(set.transitions.fade.ms().to_string()));
    g.set_t_kinds(bools(KINDS.iter().map(|k| set.transitions.kinds.contains(k)).collect()));
    g.set_e_policy(SharedString::new());
    g.set_e_fade(SharedString::new());
}

fn policy_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    let mut errors = Vec::new();
    let mut dur = |label: &str, v: SharedString, required: bool| -> Option<Option<Dur>> {
        let t = v.trim().to_string();
        if t.is_empty() {
            if required {
                errors.push(format!("{label}: укажите значение"));
                return None;
            }
            return Some(None);
        }
        match t.parse::<Dur>() {
            Ok(d) => Some(Some(d)),
            Err(_) => {
                errors.push(format!("{label}: формат ЧЧ:ММ:СС"));
                None
            }
        }
    };
    let ads_interval = dur("Интервал рекламы", g.get_p_ads_interval(), false);
    let lookahead = dur("Горизонт анонсов", g.get_p_promo_lookahead(), true);
    let promo_interval = dur("Повтор анонса", g.get_p_promo_interval(), true);
    let filler_min = dur("Минимальная пустота", g.get_p_filler_min(), true);
    let filler_interval = dur("Повтор заполнения", g.get_p_filler_interval(), true);
    let max_txt = g.get_p_ads_max().trim().to_string();
    let max: Option<Option<u32>> = if max_txt.is_empty() { Some(None) } else { max_txt.parse().ok().map(Some) };
    if max.is_none() {
        errors.push("Блоков в сутки: целое число".into());
    }
    let (cats, promos) = (split(&g.get_p_filler_cats()), g.get_p_filler_promos());
    g.set_e_policy(s(errors.join(" · ")));
    mutate(c, Some("policy"), move |set| {
        if let Some(m) = max {
            set.ads.max_per_day = m;
        }
        if let Some(d) = ads_interval {
            set.ads.min_interval = d;
        }
        if let Some(Some(d)) = lookahead {
            set.promos.lookahead = d;
        }
        if let Some(Some(d)) = promo_interval {
            set.promos.min_interval = d;
        }
        if let Some(Some(d)) = filler_min {
            set.filler.min_fill = d;
        }
        if let Some(Some(d)) = filler_interval {
            set.filler.min_interval = d;
        }
        set.filler.categories = cats;
        set.filler.use_promos = promos;
    });
    list_and_flags(c);
}

fn transitions_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Rules>();
    match g.get_t_fade().trim().parse::<i64>() {
        Ok(ms) if (0..=5000).contains(&ms) => {
            g.set_e_fade(SharedString::new());
            mutate(c, Some("fade"), move |set| set.transitions.fade = Dur(ms));
            list_and_flags(c);
        }
        _ => g.set_e_fade(s("Число от 0 до 5000")),
    }
}

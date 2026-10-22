//! Broadcast grid: schedule / template draft with undo, block editing,
//! weekday mapping, day timeline from the (draft) plan with as-run lane,
//! multi-day overview.

use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate, Weekday};
use serde_json::json;
use slint::{ComponentHandle, Model, SharedString, VecModel};

use lb_core::history::AsRunRecord;
use lb_core::rundown::Rundown;
use lb_core::schedule::{Block, BlockSource, DayTemplate, LatePolicy, Schedule, ScheduleSet};
use lb_core::{Dur, WallTime};

use super::picker::{self, Target};
use super::{Ctx, vec_model, with};
use crate::logic::timeline::{self, Segment};
use crate::logic::undo::History;
use crate::shell::{AppWindow, BlockDetails, BlockRow, DayChip, DayRow, Grid, TimelineSegment};

const WEEK: [(Weekday, &str); 7] = [
    (Weekday::Mon, "Пн"),
    (Weekday::Tue, "Вт"),
    (Weekday::Wed, "Ср"),
    (Weekday::Thu, "Чт"),
    (Weekday::Fri, "Пт"),
    (Weekday::Sat, "Сб"),
    (Weekday::Sun, "Вс"),
];
const DAYS: [i64; 3] = [3, 7, 14];

pub struct ScheduleState {
    saved: Option<ScheduleSet>,
    draft: Option<ScheduleSet>,
    history: History<ScheduleSet>,
    sched: usize,
    tpl: usize,
    block: Option<usize>,
    date: NaiveDate,
    plan: Rundown,
    asrun: Vec<AsRunRecord>,
    /// Block id behind each timeline segment.
    seg_blocks: Vec<Option<String>>,
    plan_seq: u64,
    rows: std::rc::Rc<VecModel<BlockRow>>,
    last_edit: Option<(usize, String, Instant)>,
}

impl Default for ScheduleState {
    fn default() -> Self {
        ScheduleState {
            saved: None,
            draft: None,
            history: History::default(),
            sched: 0,
            tpl: 0,
            block: None,
            date: Local::now().date_naive(),
            plan: Rundown::default(),
            asrun: Vec::new(),
            seg_blocks: Vec::new(),
            plan_seq: 0,
            rows: Default::default(),
            last_edit: None,
        }
    }
}

impl ScheduleState {
    pub fn dirty(&self) -> bool {
        self.draft.is_some() && self.draft != self.saved
    }
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Grid>();
    g.on_select_schedule(|i| {
        with(move |c| {
            c.grid.sched = i.max(0) as usize;
            c.grid.tpl = 0;
            c.grid.block = None;
            refresh(c);
            replan(c);
        })
    });
    g.on_select_template(|i| {
        with(move |c| {
            c.grid.tpl = i.max(0) as usize;
            c.grid.block = None;
            refresh(c);
        })
    });
    g.on_toggle_day(|i| with(move |c| toggle_day(c, i as usize)));
    g.on_set_enabled(|on| {
        with(move |c| {
            let s = c.grid.sched;
            mutate(c, None, move |set| {
                if let Some(x) = set.schedules.get_mut(s) {
                    x.enabled = on;
                }
            })
        })
    });
    g.on_block_edited(|i, field, value| with(move |c| block_edited(c, i as usize, field.as_str(), value.as_str())));
    g.on_block_select(|i| {
        with(move |c| {
            c.grid.block = Some(i as usize);
            details(c);
            mark_rows(c);
        })
    });
    g.on_add_block(|fixed| with(move |c| add_block(c, fixed)));
    g.on_remove_block(|i| with(move |c| remove_block(c, i as usize)));
    g.on_pick(|what, i| {
        with(move |c| {
            let i = i as usize;
            let (t, title) = match what.as_str() {
                "intro" => (Target::BlockIntro(i), "Заставка в начале блока"),
                "outro" => (Target::BlockOutro(i), "Заставка в конце блока"),
                _ => (Target::BlockMedia(i), "Ролики блока"),
            };
            picker::open(c, t, title);
        })
    });
    g.on_segment_clicked(|i| with(move |c| segment_clicked(c, i as usize)));
    g.on_segment_moved(|i, m| with(move |c| segment_moved(c, i as usize, m)));
    g.on_shift_date(|d| {
        with(move |c| {
            c.grid.date += chrono::Duration::days(d as i64);
            replan(c);
        })
    });
    g.on_today(|| with(today));
    g.on_goto_time(|t| with(move |c| goto_time(c, t.as_str())));
    g.on_view_changed(|| {});
    g.on_mode_changed(|| with(replan));
    g.on_save(|| with(save));
    g.on_revert(|| {
        with(|c| {
            let saved = c.grid.saved.clone();
            mutate(c, None, move |set| {
                if let Some(s) = saved {
                    *set = s;
                }
            });
        })
    });
    g.on_undo(|| with(undo));
    g.on_redo(|| with(redo));
    g.on_menu_action(|a| with(move |c| menu_action(c, a.as_str())));
}

/// Fresh schedules from the server.
pub fn reloaded(c: &mut Ctx) {
    let was_dirty = c.grid.dirty();
    c.grid.saved = Some(c.model.schedules.clone());
    if !was_dirty {
        c.grid.draft = c.grid.saved.clone();
    }
    refresh(c);
    if c.section() == "schedule" {
        replan(c);
    }
}

/// Screen opened.
pub fn opened(c: &mut Ctx) {
    if c.grid.draft.is_none() {
        c.grid.saved = Some(c.model.schedules.clone());
        c.grid.draft = c.grid.saved.clone();
    }
    refresh(c);
    replan(c);
}

fn template(set: &ScheduleSet, sched: usize, tpl: usize) -> Option<&DayTemplate> {
    set.schedules.get(sched)?.templates.get(tpl)
}

/// Apply an edit with undo. `coalesce` merges repeated edits of one field.
fn mutate(c: &mut Ctx, coalesce: Option<(usize, String)>, f: impl FnOnce(&mut ScheduleSet)) {
    let Some(before) = c.grid.draft.clone() else { return };
    let mut after = before.clone();
    f(&mut after);
    let merge = match (&coalesce, &c.grid.last_edit) {
        (Some((i, field)), Some((li, lf, at))) => i == li && field == lf && at.elapsed() < Duration::from_millis(1500),
        _ => false,
    };
    if !merge {
        c.grid.history.record(&before, &after);
    }
    c.grid.last_edit = coalesce.map(|(i, f)| (i, f, Instant::now()));
    c.grid.draft = Some(after);
    flags(c);
    schedule_replan(c);
}

fn flags(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Grid>();
    g.set_dirty(c.grid.dirty());
    g.set_can_undo(c.grid.history.can_undo());
    g.set_can_redo(c.grid.history.can_redo());
}

/// Rebuild selectors, weekday chips and block rows from the draft.
pub fn refresh(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Grid>();
    let set = c.grid.draft.clone().unwrap_or_default();
    let has = !set.schedules.is_empty();
    c.grid.sched = c.grid.sched.min(set.schedules.len().saturating_sub(1));
    g.set_has_schedule(has);
    g.set_schedules(vec_model(
        set.schedules.iter().map(|s| SharedString::from(if s.name.is_empty() { s.id.clone() } else { format!("{} ({})", s.name, s.id) })).collect(),
    ));
    g.set_schedule_index(if has { c.grid.sched as i32 } else { -1 });
    g.set_playlists(vec_model(c.model.playlists.iter().map(|p| SharedString::from(p.id.clone())).collect()));
    g.set_ad_breaks(vec_model(c.model.rules.ad_breaks.iter().map(|b| SharedString::from(b.id.clone())).collect()));
    let Some(s) = set.schedules.get(c.grid.sched) else {
        g.set_templates(vec_model(Vec::new()));
        g.set_days(vec_model(Vec::new()));
        c.grid.rows = std::rc::Rc::new(VecModel::from(Vec::new()));
        g.set_blocks(c.grid.rows.clone().into());
        g.set_schedule_info("".into());
        flags(c);
        return;
    };
    g.set_schedule_enabled(s.enabled);
    let forced = c.model.status.force_schedule.as_deref() == Some(s.id.as_str());
    g.set_schedule_info(
        match s.activate_at {
            Some(t) => format!("Вступает в силу {}{}", t.format("%d.%m.%Y %H:%M"), if forced { " · выбрано оператором" } else { "" }),
            None => format!("Действует сразу{}", if forced { " · выбрано оператором" } else { "" }),
        }
        .into(),
    );
    c.grid.tpl = c.grid.tpl.min(s.templates.len().saturating_sub(1));
    g.set_templates(vec_model(
        s.templates.iter().map(|t| SharedString::from(if t.name.is_empty() { t.id.clone() } else { format!("{} ({})", t.name, t.id) })).collect(),
    ));
    g.set_template_index(if s.templates.is_empty() { -1 } else { c.grid.tpl as i32 });
    let tpl_id = s.templates.get(c.grid.tpl).map(|t| t.id.clone()).unwrap_or_default();
    g.set_days(vec_model(
        WEEK.iter()
            .map(|(wd, label)| {
                let slot = s.week.slot(*wd).clone();
                let effective = slot.clone().or_else(|| s.week.default.clone()).unwrap_or_default();
                let on = effective == tpl_id;
                DayChip { label: (*label).into(), template: effective.into(), on }
            })
            .collect(),
    ));
    rows(c);
    details(c);
    flags(c);
}

fn source_parts(c: &Ctx, src: &BlockSource) -> (i32, i32, String) {
    match src {
        BlockSource::Playlist { playlist, .. } => (0, c.model.playlists.iter().position(|p| &p.id == playlist).map(|i| i as i32).unwrap_or(-1), String::new()),
        BlockSource::Media { items } => (1, -1, items.join(", ")),
        BlockSource::Category { category, .. } => (2, -1, category.clone()),
        BlockSource::AdBreak { ad_break } => (3, c.model.rules.ad_breaks.iter().position(|b| &b.id == ad_break).map(|i| i as i32).unwrap_or(-1), String::new()),
    }
}

fn block_row(c: &Ctx, i: usize, b: &Block) -> BlockRow {
    let (st, si, text) = source_parts(c, &b.source);
    let problem = match &b.source {
        BlockSource::Playlist { playlist, .. } if !c.model.playlists.iter().any(|p| &p.id == playlist) => "плейлист не найден",
        BlockSource::AdBreak { ad_break } if !c.model.rules.ad_breaks.iter().any(|x| &x.id == ad_break) => "рекламный блок не найден",
        _ => "",
    };
    BlockRow {
        index: i as i32,
        id: b.id.clone().into(),
        start: b.start.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default().into(),
        start_error: SharedString::new(),
        title: b.title.clone().into(),
        source_type: st,
        source_index: si,
        source_text: text.into(),
        late: match b.late_policy {
            LatePolicy::Strict => 0,
            LatePolicy::Finish => 1,
            LatePolicy::Truncate => 2,
        },
        truncate: b.allow_truncate,
        fixed: b.start.is_some(),
        problem: problem.into(),
    }
}

/// Rebuild the block rows (structural changes only: keeps text focus otherwise).
fn rows(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let set = c.grid.draft.clone().unwrap_or_default();
    let rows: Vec<BlockRow> =
        template(&set, c.grid.sched, c.grid.tpl).map(|t| t.blocks.iter().enumerate().map(|(i, b)| block_row(c, i, b)).collect()).unwrap_or_default();
    c.grid.rows = std::rc::Rc::new(VecModel::from(rows));
    ui.global::<Grid>().set_blocks(c.grid.rows.clone().into());
    mark_rows(c);
}

fn mark_rows(c: &mut Ctx) {
    if let Some(ui) = c.window() {
        ui.global::<Grid>().set_selected_block(c.grid.block.map(|i| i as i32).unwrap_or(-1));
    }
}

/// Refresh one row in place (after an inline edit).
fn update_row(c: &mut Ctx, i: usize, start_error: &str) {
    let set = c.grid.draft.clone().unwrap_or_default();
    if let Some(b) = template(&set, c.grid.sched, c.grid.tpl).and_then(|t| t.blocks.get(i)) {
        let mut row = block_row(c, i, b);
        if !start_error.is_empty() {
            if let Some(old) = c.grid.rows.row_data(i) {
                row.start = old.start;
            }
            row.start_error = start_error.into();
        }
        if i < c.grid.rows.row_count() {
            c.grid.rows.set_row_data(i, row);
        }
    }
}

fn details(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let set = c.grid.draft.clone().unwrap_or_default();
    let b = c.grid.block.and_then(|i| Some((i, template(&set, c.grid.sched, c.grid.tpl)?.blocks.get(i)?)));
    ui.global::<Grid>().set_details(match b {
        Some((i, b)) => BlockDetails {
            has: true,
            index: i as i32,
            id: b.id.clone().into(),
            max_duration: b.max_duration.map(|d| d.to_string()).unwrap_or_default().into(),
            max_items: b.max_items.map(|n| n.to_string()).unwrap_or_default().into(),
            category: b.category.clone().unwrap_or_default().into(),
            intro: b.intro.clone().unwrap_or_default().into(),
            outro: b.outro.clone().unwrap_or_default().into(),
            fill_gap: b.fill_gap,
            rules: b.rules,
            restart: matches!(b.source, BlockSource::Playlist { restart: true, .. }),
            late: match b.late_policy {
                LatePolicy::Strict => 0,
                LatePolicy::Finish => 1,
                LatePolicy::Truncate => 2,
            },
            truncate: b.allow_truncate,
        },
        None => BlockDetails::default(),
    });
}

fn with_block(c: &mut Ctx, i: usize, field: &str, f: impl FnOnce(&mut Block)) {
    let (s, t) = (c.grid.sched, c.grid.tpl);
    mutate(c, Some((i, field.to_string())), move |set| {
        if let Some(b) = set.schedules.get_mut(s).and_then(|x| x.templates.get_mut(t)).and_then(|t| t.blocks.get_mut(i)) {
            f(b);
        }
    });
}

fn opt(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn block_edited(c: &mut Ctx, i: usize, field: &str, v: &str) {
    match field {
        "start" => match timeline::parse_time(v) {
            Ok(t) => {
                with_block(c, i, field, move |b| b.start = t);
                update_row(c, i, "");
            }
            Err(e) => update_row(c, i, e),
        },
        "title" => with_block(c, i, field, |b| b.title = v.to_string()),
        "source-type" => {
            let first_pl = c.model.playlists.first().map(|p| p.id.clone()).unwrap_or_default();
            let first_ab = c.model.rules.ad_breaks.first().map(|b| b.id.clone()).unwrap_or_default();
            let src = match v {
                "0" => BlockSource::Playlist { playlist: first_pl, restart: false },
                "1" => BlockSource::Media { items: vec![] },
                "2" => BlockSource::Category { category: String::new(), kind: None },
                _ => BlockSource::AdBreak { ad_break: first_ab },
            };
            with_block(c, i, field, move |b| b.source = src);
            update_row(c, i, "");
        }
        "source-index" => {
            let idx: usize = v.parse().unwrap_or(0);
            let pl = c.model.playlists.get(idx).map(|p| p.id.clone());
            let ab = c.model.rules.ad_breaks.get(idx).map(|b| b.id.clone());
            with_block(c, i, field, move |b| match &mut b.source {
                BlockSource::Playlist { playlist, .. } => {
                    if let Some(p) = pl {
                        *playlist = p;
                    }
                }
                BlockSource::AdBreak { ad_break } => {
                    if let Some(a) = ab {
                        *ad_break = a;
                    }
                }
                _ => {}
            });
            update_row(c, i, "");
        }
        "source-text" => with_block(c, i, field, |b| match &mut b.source {
            BlockSource::Media { items } => *items = v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
            BlockSource::Category { category, .. } => *category = v.trim().to_string(),
            _ => {}
        }),
        "late" => {
            let p = match v {
                "1" => LatePolicy::Finish,
                "2" => LatePolicy::Truncate,
                _ => LatePolicy::Strict,
            };
            with_block(c, i, field, move |b| b.late_policy = p);
        }
        "truncate" => with_block(c, i, field, |b| b.allow_truncate = v == "1"),
        "max-duration" => {
            let d: Option<Option<Dur>> = if v.trim().is_empty() { Some(None) } else { v.trim().parse().ok().map(Some) };
            if let Some(d) = d {
                with_block(c, i, field, move |b| b.max_duration = d);
            }
        }
        "max-items" => {
            let n: Option<Option<usize>> = if v.trim().is_empty() { Some(None) } else { v.trim().parse().ok().map(Some) };
            if let Some(n) = n {
                with_block(c, i, field, move |b| b.max_items = n);
            }
        }
        "category" => with_block(c, i, field, |b| b.category = opt(v)),
        "intro" => with_block(c, i, field, |b| b.intro = opt(v)),
        "outro" => with_block(c, i, field, |b| b.outro = opt(v)),
        "fill-gap" => with_block(c, i, field, |b| b.fill_gap = v == "1"),
        "rules" => with_block(c, i, field, |b| b.rules = v == "1"),
        "restart" => with_block(c, i, field, |b| {
            if let BlockSource::Playlist { restart, .. } = &mut b.source {
                *restart = v == "1";
            }
        }),
        _ => {}
    }
    details(c);
}

fn add_block(c: &mut Ctx, fixed: bool) {
    let (s, t) = (c.grid.sched, c.grid.tpl);
    let pl = c.model.playlists.first().map(|p| p.id.clone()).unwrap_or_default();
    let mut new_index = 0;
    mutate(c, None, |set| {
        if set.schedules.is_empty() {
            set.schedules.push(new_schedule(set));
        }
        let si = s.min(set.schedules.len() - 1);
        let Some(sch) = set.schedules.get_mut(si) else { return };
        if sch.templates.is_empty() {
            sch.templates.push(DayTemplate { id: "day".into(), name: "Будни".into(), blocks: vec![] });
            sch.week.default = Some("day".into());
        }
        let ti = t.min(sch.templates.len() - 1);
        let Some(tpl) = sch.templates.get_mut(ti) else { return };
        let n = tpl.blocks.len() + 1;
        let mut id = format!("block-{n}");
        let mut k = n;
        while tpl.blocks.iter().any(|b| b.id == id) {
            k += 1;
            id = format!("block-{k}");
        }
        let start = fixed.then(|| {
            let last = tpl.blocks.iter().filter_map(|b| b.start).max();
            last.map(|l| l + chrono::Duration::hours(1)).unwrap_or_else(|| chrono::NaiveTime::from_hms_opt(6, 0, 0).unwrap_or_default())
        });
        let mut b = Block::new(&id, start, BlockSource::Playlist { playlist: pl, restart: false });
        b.title = "Новый блок".into();
        tpl.blocks.push(b);
        tpl.blocks.sort_by_key(|b| b.start.map(|t| timeline::minutes_of(t) as i64).unwrap_or(i64::MAX));
        new_index = tpl.blocks.iter().position(|b| b.id == id).unwrap_or(0);
    });
    c.grid.block = Some(new_index);
    refresh(c);
}

/// Result of the library picker.
pub fn picked(c: &mut Ctx, target: Target, ids: Vec<String>) {
    let (i, field) = match target {
        Target::BlockMedia(i) => (i, "source-text"),
        Target::BlockIntro(i) => (i, "intro"),
        Target::BlockOutro(i) => (i, "outro"),
        _ => return,
    };
    with_block(c, i, field, move |b| match target {
        Target::BlockMedia(_) => {
            if let BlockSource::Media { items } = &mut b.source {
                items.extend(ids);
            }
        }
        Target::BlockIntro(_) => b.intro = ids.into_iter().next(),
        Target::BlockOutro(_) => b.outro = ids.into_iter().next(),
        _ => {}
    });
    update_row(c, i, "");
    details(c);
}

fn remove_block(c: &mut Ctx, i: usize) {
    let (s, t) = (c.grid.sched, c.grid.tpl);
    let mut title = String::new();
    mutate(c, None, |set| {
        if let Some(tpl) = set.schedules.get_mut(s).and_then(|x| x.templates.get_mut(t))
            && i < tpl.blocks.len()
        {
            let b = tpl.blocks.remove(i);
            title = if b.title.is_empty() { b.id } else { b.title };
        }
    });
    c.grid.block = None;
    refresh(c);
    c.toast_with(format!("Блок «{title}» удалён"), "Отменить", undo);
}

/// Assign the current template to a weekday, or release the day back to
/// the default template.
fn toggle_day(c: &mut Ctx, i: usize) {
    let Some((wd, label)) = WEEK.get(i).copied() else { return };
    let (s, t) = (c.grid.sched, c.grid.tpl);
    let Some(sch) = c.grid.draft.as_ref().and_then(|d| d.schedules.get(s)) else { return };
    let Some(id) = sch.templates.get(t).map(|x| x.id.clone()) else { return };
    let slot = sch.week.slot(wd).clone();
    let default = sch.week.default.clone();
    let assigned = slot.as_deref().or(default.as_deref()) == Some(id.as_str());
    if assigned && slot.is_none() {
        c.toast(format!("{label}: шаблон «{id}» используется по умолчанию — выберите для этого дня другой шаблон и отметьте день"), false);
        return;
    }
    mutate(c, None, move |set| {
        if let Some(sch) = set.schedules.get_mut(s) {
            *sch.week.slot_mut(wd) = if assigned { None } else { Some(id) };
        }
    });
    refresh(c);
}

fn undo(c: &mut Ctx) {
    let Some(cur) = c.grid.draft.clone() else { return };
    if let Some(prev) = c.grid.history.undo(&cur) {
        c.grid.draft = Some(prev);
        c.grid.last_edit = None;
        refresh(c);
        schedule_replan(c);
    }
}

fn redo(c: &mut Ctx) {
    let Some(cur) = c.grid.draft.clone() else { return };
    if let Some(next) = c.grid.history.redo(&cur) {
        c.grid.draft = Some(next);
        c.grid.last_edit = None;
        refresh(c);
        schedule_replan(c);
    }
}

fn save(c: &mut Ctx) {
    if !c.grid.dirty() {
        return;
    }
    if let Some(set) = c.grid.draft.clone() {
        c.save_doc("/schedules", set, "Сетка вещания");
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

/// An empty schedule with one day template used for every day.
fn new_schedule(set: &ScheduleSet) -> Schedule {
    let mut n = set.schedules.len() + 1;
    let id = loop {
        let id = if n == 1 { "main".to_string() } else { format!("schedule-{n}") };
        if !set.schedules.iter().any(|x| x.id == id) {
            break id;
        }
        n += 1;
    };
    Schedule {
        id,
        name: if n == 1 { "Основное".into() } else { format!("Расписание {n}") },
        enabled: true,
        week: lb_core::schedule::WeekMap { default: Some("day".into()), ..Default::default() },
        templates: vec![DayTemplate { id: "day".into(), name: "Каждый день".into(), blocks: vec![] }],
        ..Default::default()
    }
}

fn menu_action(c: &mut Ctx, a: &str) {
    if c.grid.draft.is_none() {
        c.grid.draft = Some(c.model.schedules.clone());
    }
    let (s, t) = (c.grid.sched, c.grid.tpl);
    match a {
        "new-schedule" => {
            mutate(c, None, |set| {
                let sch = new_schedule(set);
                set.schedules.push(sch);
            });
            c.grid.sched = c.grid.draft.as_ref().map(|d| d.schedules.len().saturating_sub(1)).unwrap_or(0);
            c.grid.tpl = 0;
            c.grid.block = None;
            refresh(c);
            c.toast("Расписание создано — добавьте блоки и сохраните", false);
        }
        "new-template" => {
            mutate(c, None, |set| {
                if let Some(sch) = set.schedules.get_mut(s) {
                    let mut n = sch.templates.len() + 1;
                    while sch.templates.iter().any(|x| x.id == format!("day-{n}")) {
                        n += 1;
                    }
                    sch.templates.push(DayTemplate { id: format!("day-{n}"), name: format!("Шаблон {n}"), blocks: vec![] });
                }
            });
            c.grid.tpl = c.grid.draft.as_ref().and_then(|d| d.schedules.get(s)).map(|x| x.templates.len().saturating_sub(1)).unwrap_or(0);
            refresh(c);
        }
        "duplicate-template" => {
            mutate(c, None, |set| {
                if let Some(sch) = set.schedules.get_mut(s)
                    && let Some(from) = sch.templates.get(t).map(|x| x.id.clone())
                {
                    let mut n = 2;
                    while sch.templates.iter().any(|x| x.id == format!("{from}-{n}")) {
                        n += 1;
                    }
                    sch.duplicate_template(&from, &format!("{from}-{n}"));
                }
            });
            c.grid.tpl = c.grid.draft.as_ref().and_then(|d| d.schedules.get(s)).map(|x| x.templates.len().saturating_sub(1)).unwrap_or(0);
            refresh(c);
        }
        "replan" => replan(c),
        _ => {}
    }
}

// ---------------------------------------------------------------- timeline

/// Re-plan shortly after the last edit (typing should not flood the server).
fn schedule_replan(c: &mut Ctx) {
    c.grid.plan_seq += 1;
    let seq = c.grid.plan_seq;
    slint::Timer::single_shot(Duration::from_millis(600), move || {
        with(move |c| {
            if c.grid.plan_seq == seq {
                replan(c);
            }
        })
    });
}

fn day_start(d: NaiveDate) -> WallTime {
    d.and_hms_opt(0, 0, 0).unwrap_or_default()
}

/// Fetch the plan for the visible range (from the draft when it differs
/// from the server) and the as-run log for past days.
pub fn replan(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    if c.model.ch().is_empty() {
        return;
    }
    let g = ui.global::<Grid>();
    let many = g.get_mode() == 1;
    let today = Local::now().date_naive();
    let (from, hours) = if many {
        (day_start(today), 24 * DAYS[(g.get_days_count_index().clamp(0, 2)) as usize])
    } else {
        (day_start(c.grid.date), 24)
    };
    g.set_planning(true);
    c.grid.plan_seq += 1;
    let seq = c.grid.plan_seq;
    let ch = c.model.ch();
    let draft = c.grid.dirty().then(|| c.grid.draft.clone()).flatten();
    let date = c.grid.date;
    let want_asrun = !many && date <= today;
    c.worker.spawn(move |api| {
        let from_s = from.format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut note = String::new();
        let plan: Result<Rundown, String> = match &draft {
            Some(set) => match api.post(&format!("/channels/{ch}/plan"), &json!({ "from": from_s, "hours": hours, "schedules": set })) {
                Ok(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
                Err(_) => {
                    note = "Прогноз по сохранённой сетке: сервер не поддерживает предпросмотр черновика".into();
                    api.get(&format!("/channels/{ch}/plan?from={from_s}&hours={hours}"))
                }
            },
            None => api.get(&format!("/channels/{ch}/plan?from={from_s}&hours={hours}")),
        };
        let asrun: Vec<AsRunRecord> =
            if want_asrun { api.get(&format!("/channels/{ch}/asrun?from={date}&to={date}")).unwrap_or_default() } else { Vec::new() };
        Ok(crate::net::Msg::Apply(Box::new(move |c: &mut Ctx| {
            if c.grid.plan_seq != seq {
                return;
            }
            if let Some(ui) = c.window() {
                let g = ui.global::<Grid>();
                g.set_planning(false);
                g.set_plan_note(note.into());
            }
            match plan {
                Ok(p) => {
                    c.grid.plan = p;
                    c.grid.asrun = asrun;
                    timeline_changed(c);
                }
                Err(e) => c.toast(format!("Прогноз эфира: {e}"), true),
            }
        })))
    });
}

fn to_ui(s: &Segment, lane: i32, selected: bool, movable: bool) -> TimelineSegment {
    TimelineSegment {
        start: s.start,
        duration: s.duration,
        title: s.title.clone().into(),
        subtitle: s.subtitle.clone().into(),
        kind: s.kind.clone().into(),
        lane,
        gap: s.gap,
        conflict: s.conflict,
        current: s.current,
        selected,
        movable,
    }
}

fn timeline_changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Grid>();
    let now = Local::now().naive_local();
    let today = now.date();
    let p = &c.grid.plan;
    let gaps = p.items.iter().filter(|i| i.is_slate()).count();
    g.set_gaps(gaps as i32);
    g.set_conflicts(p.warnings.len() as i32);
    g.set_warnings(vec_model(
        p.warnings
            .iter()
            .map(|w| SharedString::from(format!("{} {}", w.at.map(|t| t.format("%d.%m %H:%M").to_string()).unwrap_or_default(), w.message)))
            .collect(),
    ));
    if g.get_mode() == 1 {
        let n = DAYS[(g.get_days_count_index().clamp(0, 2)) as usize];
        let rows: Vec<DayRow> = (0..n)
            .map(|d| {
                let day = today + chrono::Duration::days(d);
                let segs = timeline::segments(p, day, Some(now));
                DayRow {
                    label: if d == 0 { "Сегодня".into() } else { format!("{} {}", WEEK[day.weekday().num_days_from_monday() as usize].1, day.format("%d")).into() },
                    segments: vec_model(segs.iter().map(|s| to_ui(s, 0, false, false)).collect()),
                }
            })
            .collect();
        g.set_day_rows(vec_model(rows));
        g.set_date_label(format!("{} — {}", today.format("%d.%m"), (today + chrono::Duration::days(n - 1)).format("%d.%m.%Y")).into());
        g.set_now(crate::logic::timeline::minutes_of(now.time()));
        return;
    }
    let day = c.grid.date;
    let segs = timeline::segments(p, day, Some(now));
    let set = c.grid.draft.clone().unwrap_or_default();
    let tpl = set.schedules.get(c.grid.sched).and_then(|s| s.template_for(day));
    let sel_id = c.grid.block.and_then(|i| template(&set, c.grid.sched, c.grid.tpl).and_then(|t| t.blocks.get(i)).map(|b| b.id.clone()));
    let mut ui_segs: Vec<TimelineSegment> = segs
        .iter()
        .map(|s| {
            let fixed = s.block_id.as_ref().is_some_and(|id| tpl.is_some_and(|t| t.blocks.iter().any(|b| &b.id == id && b.start.is_some())));
            let selected = s.block_id.is_some() && s.block_id == sel_id && !s.gap;
            to_ui(s, 0, selected, fixed && !s.gap)
        })
        .collect();
    c.grid.seg_blocks = segs.iter().map(|s| s.block_id.clone()).collect();
    ui_segs.extend(timeline::actual(&c.grid.asrun, day).iter().map(|s| to_ui(s, 1, false, false)));
    c.grid.seg_blocks.resize(ui_segs.len(), None);
    g.set_segments(vec_model(ui_segs));
    let weekday = WEEK[day.weekday().num_days_from_monday() as usize].1;
    g.set_date_label(
        format!(
            "{weekday}, {}{}{}",
            day.format("%d.%m.%Y"),
            if day == today { " · сегодня" } else { "" },
            tpl.map(|t| format!(" · шаблон «{}»", if t.name.is_empty() { &t.id } else { &t.name })).unwrap_or_default()
        )
        .into(),
    );
    g.set_now(if day == today { timeline::minutes_of(now.time()) } else { -1.0 });
}

/// Segment selected on the timeline → select the block (and its template).
fn segment_clicked(c: &mut Ctx, i: usize) {
    let Some(Some(block_id)) = c.grid.seg_blocks.get(i).cloned() else { return };
    let set = c.grid.draft.clone().unwrap_or_default();
    let Some(sch) = set.schedules.get(c.grid.sched) else { return };
    let Some(tpl) = sch.template_for(c.grid.date) else { return };
    if let Some(ti) = sch.templates.iter().position(|t| t.id == tpl.id) {
        c.grid.tpl = ti;
    }
    c.grid.block = tpl.blocks.iter().position(|b| b.id == block_id);
    refresh(c);
    timeline_changed(c);
}

/// Fixed block dragged on the timeline → new start time (5 min snap).
fn segment_moved(c: &mut Ctx, i: usize, minutes: f32) {
    segment_clicked(c, i);
    let Some(bi) = c.grid.block else { return };
    let t = timeline::time_of(minutes, 5.0);
    with_block(c, bi, "drag", move |b| {
        if b.start.is_some() {
            b.start = Some(t);
        }
    });
    c.grid.last_edit = None;
    if let Some(tpl) = c.grid.draft.as_mut().and_then(|d| d.schedules.get_mut(c.grid.sched)).and_then(|s| s.templates.get_mut(c.grid.tpl)) {
        tpl.blocks.sort_by_key(|b| b.start.map(|t| timeline::minutes_of(t) as i64).unwrap_or(i64::MAX));
    }
    refresh(c);
    c.toast(format!("Начало блока: {}", t.format("%H:%M")), false);
}

fn today(c: &mut Ctx) {
    c.grid.date = Local::now().date_naive();
    if let Some(ui) = c.window() {
        let g = ui.global::<Grid>();
        let span = g.get_view_span().min(360.0);
        let now = timeline::minutes_of(Local::now().time());
        g.set_view_span(span);
        g.set_view_start((now - span / 3.0).clamp(0.0, 1440.0 - span));
        g.set_mode(0);
    }
    replan(c);
}

fn goto_time(c: &mut Ctx, t: &str) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Grid>();
    match timeline::parse_time(t) {
        Ok(Some(t)) => {
            let span = g.get_view_span();
            g.set_view_start((timeline::minutes_of(t) - span / 2.0).clamp(0.0, (1440.0 - span).max(0.0)));
            g.set_goto_error(SharedString::new());
        }
        Ok(None) => g.set_goto_error(SharedString::new()),
        Err(e) => g.set_goto_error(e.into()),
    }
}

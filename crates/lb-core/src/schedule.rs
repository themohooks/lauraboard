//! Broadcast grid: day templates, weekly mapping, date exceptions and
//! schedules that activate at a given moment.

use chrono::{Datelike, NaiveDate, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::media::MediaKind;
use crate::{Dur, WallTime};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockSource {
    /// Pull items from a playlist. The cursor is kept between instances
    /// unless `restart` is set (then each instance starts from the top).
    Playlist {
        playlist: String,
        #[serde(default)]
        restart: bool,
    },
    /// Fixed list of media ids, played once.
    Media { items: Vec<String> },
    /// Random items of a category from the library.
    Category {
        category: String,
        #[serde(default)]
        kind: Option<MediaKind>,
    },
    /// A configured ad break.
    AdBreak { ad_break: String },
}

/// What to do when a block cannot end before the next fixed-time block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LatePolicy {
    /// Do not start content that does not fit; drop optional interstitials;
    /// truncate only when `allow_truncate`; otherwise delay the next block.
    #[default]
    Strict,
    /// Let the last started item finish and delay the next block.
    Finish,
    /// Cut the running item at the fixed time (requires `allow_truncate`).
    Truncate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// Fixed start time; `None` means the block follows the previous one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<NaiveTime>,
    pub source: BlockSource,
    /// Maximum length (mainly for follow blocks). Fixed blocks always end at
    /// the next fixed block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration: Option<Dur>,
    /// Maximum number of main items in the block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<usize>,
    #[serde(default)]
    pub late_policy: LatePolicy,
    /// Explicit permission to cut content to honour the next fixed start.
    #[serde(default)]
    pub allow_truncate: bool,
    /// Category assigned to items of this block (overrides media category
    /// for rule matching).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Skip items aired within this period (random/category sources and
    /// playlists alike).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_recent: Option<Dur>,
    /// Apply interstitial rules inside this block.
    #[serde(default = "yes")]
    pub rules: bool,
    /// Optional explicit intro / outro media ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outro: Option<String>,
    /// Fill the remaining gap before the next fixed block with fillers.
    #[serde(default = "yes")]
    pub fill_gap: bool,
    /// Free-form info (presenter, description) for graphics templates.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub meta: HashMap<String, String>,
}

fn yes() -> bool {
    true
}

impl Block {
    pub fn new(id: &str, start: Option<NaiveTime>, source: BlockSource) -> Self {
        Block {
            id: id.to_string(),
            title: id.to_string(),
            start,
            source,
            max_duration: None,
            max_items: None,
            late_policy: LatePolicy::Strict,
            allow_truncate: false,
            category: None,
            exclude_recent: None,
            rules: true,
            intro: None,
            outro: None,
            fill_gap: true,
            meta: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DayTemplate {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WeekMap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sun: Option<String>,
    /// Used for days not set explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl WeekMap {
    pub fn slot(&self, wd: Weekday) -> &Option<String> {
        match wd {
            Weekday::Mon => &self.mon,
            Weekday::Tue => &self.tue,
            Weekday::Wed => &self.wed,
            Weekday::Thu => &self.thu,
            Weekday::Fri => &self.fri,
            Weekday::Sat => &self.sat,
            Weekday::Sun => &self.sun,
        }
    }
    pub fn slot_mut(&mut self, wd: Weekday) -> &mut Option<String> {
        match wd {
            Weekday::Mon => &mut self.mon,
            Weekday::Tue => &mut self.tue,
            Weekday::Wed => &mut self.wed,
            Weekday::Thu => &mut self.thu,
            Weekday::Fri => &mut self.fri,
            Weekday::Sat => &mut self.sat,
            Weekday::Sun => &mut self.sun,
        }
    }
    pub fn get(&self, wd: Weekday) -> Option<&str> {
        self.slot(wd).as_deref().or(self.default.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DateException {
    pub date: NaiveDate,
    /// Template to use on that date; `None` = no broadcast grid (fallback).
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Schedule {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// The schedule takes over from this moment (prepared in advance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activate_at: Option<WallTime>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub week: WeekMap,
    #[serde(default)]
    pub exceptions: Vec<DateException>,
    #[serde(default)]
    pub templates: Vec<DayTemplate>,
}

impl Schedule {
    pub fn template(&self, id: &str) -> Option<&DayTemplate> {
        self.templates.iter().find(|t| t.id == id)
    }

    pub fn template_for(&self, date: NaiveDate) -> Option<&DayTemplate> {
        if let Some(ex) = self.exceptions.iter().find(|e| e.date == date) {
            return ex.template.as_deref().and_then(|t| self.template(t));
        }
        self.week.get(date.weekday()).and_then(|t| self.template(t))
    }

    /// Assign a template to several weekdays ("copy to other days").
    pub fn assign_days(&mut self, template: &str, days: &[Weekday]) {
        for d in days {
            *self.week.slot_mut(*d) = Some(template.to_string());
        }
    }

    /// Duplicate a template under a new id.
    pub fn duplicate_template(&mut self, from: &str, new_id: &str) -> bool {
        let Some(mut t) = self.template(from).cloned() else { return false };
        t.id = new_id.to_string();
        t.name = format!("{} (copy)", t.name);
        self.templates.push(t);
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScheduleSet {
    #[serde(default)]
    pub schedules: Vec<Schedule>,
}

/// A concrete block occurrence on a date.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockInstance<'a> {
    pub schedule: &'a str,
    pub date: NaiveDate,
    pub block: &'a Block,
    /// Fixed start in wall time (None for follow blocks).
    pub fixed_start: Option<WallTime>,
}

impl<'a> BlockInstance<'a> {
    pub fn key(&self) -> String {
        format!("{}:{}", self.date, self.block.id)
    }
}

impl ScheduleSet {
    pub fn get(&self, id: &str) -> Option<&Schedule> {
        self.schedules.iter().find(|s| s.id == id)
    }

    /// Schedule active at `t`. `force` selects a schedule explicitly
    /// (operator switched schedules).
    pub fn active_at(&self, t: WallTime, force: Option<&str>) -> Option<&Schedule> {
        if let Some(f) = force {
            return self.get(f);
        }
        self.schedules.iter().filter(|s| s.enabled && s.activate_at.is_none_or(|a| a <= t)).max_by_key(|s| s.activate_at)
    }

    /// Block instances for a date in order. Instances belong to the schedule
    /// active at their (fixed) start; follow blocks inherit from the
    /// preceding fixed block.
    pub fn instances(&self, date: NaiveDate, force: Option<&str>) -> Vec<BlockInstance<'_>> {
        let mut out = Vec::new();
        let day_start = date.and_hms_opt(0, 0, 0).unwrap();
        // collect candidate schedules for that day
        let mut candidates: Vec<&Schedule> = match force {
            Some(f) => self.get(f).into_iter().collect(),
            None => self.schedules.iter().filter(|s| s.enabled).collect(),
        };
        candidates.sort_by_key(|s| s.activate_at);
        for s in candidates {
            let Some(tpl) = s.template_for(date) else { continue };
            let mut current_owner = false;
            for (bi, b) in tpl.blocks.iter().enumerate() {
                let fixed = b.start.map(|st| date.and_time(st));
                if bi == 0 || fixed.is_some() {
                    let t = fixed.unwrap_or(day_start);
                    current_owner = self.active_at(t, force).is_some_and(|a| a.id == s.id);
                }
                if current_owner {
                    out.push(BlockInstance { schedule: &s.id, date, block: b, fixed_start: fixed });
                }
            }
        }
        // stable order by fixed time; follow blocks keep position after their anchor
        let mut keyed: Vec<(WallTime, usize, BlockInstance)> = Vec::new();
        let mut last = day_start;
        for (i, inst) in out.into_iter().enumerate() {
            if let Some(f) = inst.fixed_start {
                last = f;
            }
            keyed.push((last, i, inst));
        }
        keyed.sort_by_key(|a| (a.0, a.1));
        keyed.into_iter().map(|(_, _, i)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn sched(id: &str, act: Option<WallTime>, tpl_blocks: Vec<Block>) -> Schedule {
        Schedule {
            id: id.into(),
            name: id.into(),
            activate_at: act,
            enabled: true,
            week: WeekMap { default: Some("d".into()), ..Default::default() },
            exceptions: vec![],
            templates: vec![DayTemplate { id: "d".into(), name: "d".into(), blocks: tpl_blocks }],
        }
    }

    fn pl(id: &str) -> BlockSource {
        BlockSource::Playlist { playlist: id.into(), restart: false }
    }

    #[test]
    fn weekday_and_exception() {
        let mut s = sched("s", None, vec![]);
        s.templates.push(DayTemplate { id: "we".into(), name: "weekend".into(), blocks: vec![] });
        s.assign_days("we", &[Weekday::Sat, Weekday::Sun]);
        let sat = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
        let mon = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        assert_eq!(s.template_for(sat).unwrap().id, "we");
        assert_eq!(s.template_for(mon).unwrap().id, "d");
        s.exceptions.push(DateException { date: mon, template: Some("we".into()), note: String::new() });
        assert_eq!(s.template_for(mon).unwrap().id, "we");
    }

    #[test]
    fn activation_switches_schedule() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let a = sched("a", None, vec![Block::new("a1", Some(t(6, 0)), pl("x")), Block::new("a2", Some(t(18, 0)), pl("x"))]);
        let b = sched(
            "b",
            Some(d.and_time(t(12, 0))),
            vec![Block::new("b1", Some(t(6, 0)), pl("x")), Block::new("b2", Some(t(20, 0)), pl("x"))],
        );
        let set = ScheduleSet { schedules: vec![a, b] };
        let ids: Vec<_> = set.instances(d, None).iter().map(|i| i.block.id.clone()).collect();
        assert_eq!(ids, ["a1", "b2"]);
        let forced: Vec<_> = set.instances(d, Some("a")).iter().map(|i| i.block.id.clone()).collect();
        assert_eq!(forced, ["a1", "a2"]);
    }

    #[test]
    fn follow_blocks_keep_order() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let s = sched(
            "s",
            None,
            vec![Block::new("m", Some(t(6, 0)), pl("x")), Block::new("f", None, pl("y")), Block::new("e", Some(t(9, 0)), pl("x"))],
        );
        let set = ScheduleSet { schedules: vec![s] };
        let ids: Vec<_> = set.instances(d, None).iter().map(|i| i.block.id.clone()).collect();
        assert_eq!(ids, ["m", "f", "e"]);
    }
}

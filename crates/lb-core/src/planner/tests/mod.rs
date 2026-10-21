//! Planner tests. Fixtures live here; scenarios are grouped by topic.

mod ads_promos;
mod grid;
mod rules;
mod sequential;

use chrono::{NaiveDate, NaiveTime};

use crate::history::MemHistory;
use crate::media::{Library, MediaItem, MediaKind};
use crate::playlist::Playlist;
use crate::rules::{Conditions, InsertAction, InsertionRule, RuleSet, SelectMode, Trigger};
use crate::rundown::{Origin, Rundown, RundownItem, WarningKind};
use crate::schedule::{Block, BlockSource, DayTemplate, Schedule, ScheduleSet, WeekMap};
use crate::{Dur, WallTime};

use super::{PlanInput, PlanMode, PlanState, plan};

/// Test date: Wednesday 2026-10-21.
pub fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 21).unwrap()
}

pub fn at(h: u32, m: u32) -> WallTime {
    day().and_hms_opt(h, m, 0).unwrap()
}

pub fn tm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

pub fn media(id: &str, kind: MediaKind, secs: i64) -> MediaItem {
    MediaItem::new(id, format!("media/{id}.mp4"), kind, Dur::secs(secs))
}

pub fn prog(id: &str, mins: i64) -> MediaItem {
    media(id, MediaKind::Program, mins * 60)
}

pub fn pl_src(id: &str) -> BlockSource {
    BlockSource::Playlist { playlist: id.into(), restart: false }
}

pub fn fixed(id: &str, h: u32, m: u32, src: BlockSource) -> Block {
    Block::new(id, Some(tm(h, m)), src)
}

pub fn rule(id: &str, trigger: Trigger, action: InsertAction) -> InsertionRule {
    InsertionRule {
        id: id.into(),
        name: id.into(),
        enabled: true,
        trigger,
        action,
        conditions: Conditions::default(),
        select: SelectMode::Sequential,
        probability: 1.0,
        no_repeat: true,
        priority: 0,
        group: None,
        optional: true,
        skip_on_manual: true,
    }
}

pub fn jingle_rule(id: &str, trigger: Trigger, items: &[&str]) -> InsertionRule {
    rule(id, trigger, InsertAction::Jingle { category: None, items: items.iter().map(|s| s.to_string()).collect() })
}

/// Mutable test fixture.
pub struct Fx {
    pub lib: Library,
    pub playlists: Vec<Playlist>,
    pub blocks: Vec<Block>,
    pub rules: RuleSet,
    pub history: MemHistory,
    pub mode: PlanMode,
    pub state: PlanState,
}

impl Fx {
    pub fn new() -> Self {
        let mut rules = RuleSet::default();
        rules.filler.min_fill = Dur::secs(1);
        Fx {
            lib: Library::default(),
            playlists: vec![],
            blocks: vec![],
            rules,
            history: MemHistory::new(),
            mode: PlanMode::Grid,
            state: PlanState::default(),
        }
    }

    pub fn add(&mut self, items: Vec<MediaItem>) -> &mut Self {
        for m in items {
            self.lib.upsert(m);
        }
        self
    }

    pub fn playlist(&mut self, id: &str, media: &[&str]) -> &mut Self {
        self.playlists.push(Playlist::new(id).with_media(media));
        self
    }

    pub fn schedules(&self) -> ScheduleSet {
        ScheduleSet {
            schedules: vec![Schedule {
                id: "s".into(),
                name: "s".into(),
                activate_at: None,
                enabled: true,
                week: WeekMap { default: Some("d".into()), ..Default::default() },
                exceptions: vec![],
                templates: vec![DayTemplate { id: "d".into(), name: "d".into(), blocks: self.blocks.clone() }],
            }],
        }
    }

    pub fn plan(&self, from: WallTime, until: WallTime) -> Rundown {
        let sched = self.schedules();
        let input = PlanInput {
            library: &self.lib,
            playlists: &self.playlists,
            schedules: &sched,
            rules: &self.rules,
            history: &self.history,
            mode: &self.mode,
            seed: 7,
            force_schedule: None,
        };
        plan(&input, &self.state, from, until)
    }
}

/// Media ids in order.
pub fn ids(r: &Rundown) -> Vec<&str> {
    r.items.iter().map(|i| if i.is_slate() { "<slate>" } else { i.media.as_str() }).collect()
}

/// Assert the rundown is contiguous (no gaps or overlaps).
pub fn assert_contiguous(r: &Rundown) {
    for w in r.items.windows(2) {
        assert_eq!(w[0].end(), w[1].start, "gap/overlap between '{}' and '{}'", w[0].title, w[1].title);
    }
}

pub fn first_of_block<'a>(r: &'a Rundown, block: &str) -> &'a RundownItem {
    r.items.iter().find(|i| i.block.as_deref() == Some(block) && i.origin == Origin::Main).expect("block item")
}

pub fn has_warning(r: &Rundown, k: WarningKind) -> bool {
    r.warnings.iter().any(|w| w.kind == k)
}

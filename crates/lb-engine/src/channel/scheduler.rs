//! Runtime scheduling: keeps the rundown aligned with real time, picks the
//! next item, handles the manual queue and manual mode. Pure logic (no IO)
//! so it can be tested without FFmpeg.

use std::collections::VecDeque;

use lb_core::history::{AsRunRecord, AsRunStatus, MemHistory};
use lb_core::media::MediaKind;
use lb_core::planner::{self, PlanInput, PlanRequest, PlanState};
use lb_core::project::ChannelData;
use lb_core::rundown::{Origin, Rundown, RundownItem};
use lb_core::{Ms, WallTime, add_ms, diff_ms};

/// Items starting this much before their fixed time are held back and the
/// gap is filled instead.
const EARLY_TOLERANCE_MS: Ms = 2_000;

pub struct Scheduler {
    /// Upcoming items; `items[0]` plays next. The on-air item is not here.
    pub rundown: Rundown,
    /// Committed planning state (items that actually started).
    pub plan: PlanState,
    pub history: MemHistory,
    pub manual: bool,
    pub queue: VecDeque<RundownItem>,
    pub force_schedule: Option<String>,
    /// Block of the last started item and how many main items of it aired.
    pub block_key: Option<String>,
    pub block_items_done: usize,
    /// Incremented whenever the rundown is replaced.
    pub version: u64,
    seq: u64,
}

impl Scheduler {
    pub fn new(plan: PlanState, history: MemHistory) -> Self {
        Scheduler {
            rundown: Rundown::default(),
            plan,
            history,
            manual: false,
            queue: VecDeque::new(),
            force_schedule: None,
            block_key: None,
            block_items_done: 0,
            version: 0,
            seq: 0,
        }
    }

    fn input<'a>(&'a self, data: &'a ChannelData) -> PlanInput<'a> {
        PlanInput {
            library: &data.library,
            playlists: &data.playlists,
            schedules: &data.schedules,
            rules: &data.rules,
            history: &self.history,
            mode: &data.config.planning.mode,
            seed: data.config.planning.seed,
            force_schedule: self.force_schedule.as_deref(),
        }
    }

    fn horizon(data: &ChannelData) -> Ms {
        data.config.planning.horizon.ms().max(3_600_000)
    }

    /// Full replan from `from`, continuing the block of the last started item.
    pub fn replan_hard(&mut self, data: &ChannelData, from: WallTime) {
        let mut req = PlanRequest::new(from, add_ms(from, Self::horizon(data)));
        req.current_block = self.block_key.clone();
        req.block_items_done = self.block_items_done;
        self.rundown = planner::plan_req(&self.input(data), &self.plan, &req);
        self.version += 1;
    }

    /// Replan after `current` started: keep the rest of its block as decided
    /// and plan the following blocks from the expected end.
    pub fn replan_soft(&mut self, data: &ChannelData, current: &RundownItem, expected_end: WallTime) {
        let Some(key) = current.block_key.clone() else {
            self.replan_hard(data, expected_end);
            return;
        };
        let prefix: Vec<RundownItem> =
            self.rundown.items.iter().take_while(|i| i.block_key.as_deref() == Some(key.as_str())).cloned().collect();
        let mut req = PlanRequest::new(expected_end, add_ms(expected_end, Self::horizon(data)));
        req.prefix = prefix;
        req.after_block = Some(key);
        self.rundown = planner::plan_req(&self.input(data), &self.plan, &req);
        self.version += 1;
    }

    /// Next item to air at `now`, or `None` when nothing is available
    /// (manual mode with empty queue, or empty rundown).
    pub fn take_next(&mut self, data: &ChannelData, now: WallTime) -> Option<RundownItem> {
        if let Some(mut q) = self.queue.pop_front() {
            q.start = now;
            return Some(q);
        }
        if self.manual {
            return None;
        }
        // drop items that ended in the past (engine was stopped / stalled)
        while self.rundown.items.first().is_some_and(|i| i.end() <= now && !i.is_slate() && i.fixed_start.is_none()) {
            if self.rundown.items.len() <= 1 {
                break;
            }
            self.rundown.items.remove(0);
        }
        let front = self.rundown.items.first()?;
        if front.fixed_start.is_some_and(|f| diff_ms(f, now) > EARLY_TOLERANCE_MS) {
            // early for a fixed block: fill the gap first
            self.replan_hard(data, now);
        }
        if self.rundown.items.is_empty() {
            return None;
        }
        Some(self.rundown.items.remove(0))
    }

    /// Item started on air.
    pub fn on_start(&mut self, item: &RundownItem) {
        self.plan.commit(item);
        if item.origin == Origin::Manual {
            return;
        }
        if item.block_key != self.block_key {
            self.block_key = item.block_key.clone();
            self.block_items_done = 0;
        }
        if item.origin == Origin::Main {
            self.block_items_done += 1;
        }
    }

    /// Item finished (any status).
    pub fn on_end(&mut self, rec: &AsRunRecord) {
        if rec.status != AsRunStatus::Skipped && !rec.media.is_empty() {
            self.history.record(&rec.media, rec.started);
        }
    }

    /// Item for a library media, inserted manually.
    pub fn manual_item(&mut self, data: &ChannelData, media: &str, now: WallTime) -> Option<RundownItem> {
        let m = data.library.get(media)?;
        self.seq += 1;
        Some(RundownItem {
            uid: format!("manual-{}-{}", now.format("%H%M%S"), self.seq),
            media: m.id.clone(),
            title: if m.title.is_empty() { m.id.clone() } else { m.title.clone() },
            kind: m.kind,
            origin: Origin::Manual,
            category: m.category.clone(),
            block: None,
            block_title: None,
            block_key: None,
            playlist: None,
            rule: None,
            start: now,
            duration_ms: m.play_duration().max(1000),
            seek_ms: m.in_point.map(|d| d.ms()).unwrap_or(0),
            truncated: false,
            optional: false,
            priority: 0,
            fixed_start: None,
            fade_in_ms: 0,
            fade_out_ms: 0,
            cursor_after: None,
            skip_on_manual: false,
        })
    }

    /// Unscheduled ad break items.
    pub fn ad_break_items(&self, data: &ChannelData, ad_break: &str, now: WallTime) -> Vec<RundownItem> {
        let mut items = planner::ad_break_now(&self.input(data), &self.plan, ad_break, now);
        for i in &mut items {
            i.origin = if i.is_slate() { Origin::Slate } else { Origin::Manual };
        }
        items
    }

    /// Standby item used when nothing else can play.
    pub fn standby_item(&mut self, data: &ChannelData, now: WallTime, ms: Ms) -> RundownItem {
        self.seq += 1;
        let fallback = data.config.fallback.media.as_deref().and_then(|m| data.library.get(m));
        RundownItem {
            uid: format!("standby-{}-{}", now.format("%H%M%S"), self.seq),
            media: fallback.map(|m| m.id.clone()).unwrap_or_default(),
            title: fallback.map(|m| m.title.clone()).unwrap_or_else(|| "Standby".into()),
            kind: fallback.map(|m| m.kind).unwrap_or(MediaKind::Image),
            origin: if fallback.is_some() { Origin::Filler } else { Origin::Slate },
            category: None,
            block: None,
            block_title: None,
            block_key: None,
            playlist: None,
            rule: None,
            start: now,
            duration_ms: fallback.map(|m| m.play_duration().min(ms)).filter(|d| *d > 0).unwrap_or(ms),
            seek_ms: 0,
            truncated: false,
            optional: true,
            priority: -1000,
            fixed_start: None,
            fade_in_ms: 0,
            fade_out_ms: 0,
            cursor_after: None,
            skip_on_manual: true,
        }
    }

    /// Next item that will play (queue first).
    pub fn peek_next(&self) -> Option<&RundownItem> {
        self.queue.front().or_else(|| if self.manual { None } else { self.rundown.items.first() })
    }

    /// Next main item belonging to another block than `current`.
    pub fn next_program(&self, current: Option<&RundownItem>) -> Option<&RundownItem> {
        let key = current.and_then(|c| c.block_key.clone());
        self.rundown.items.iter().find(|i| i.origin == Origin::Main && (key.is_none() || i.block_key != key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use lb_core::Dur;
    use lb_core::config::ChannelConfig;
    use lb_core::media::MediaItem;
    use lb_core::playlist::Playlist;
    use lb_core::schedule::{Block, BlockSource, DayTemplate, Schedule, ScheduleSet, WeekMap};

    fn at(h: u32, m: u32) -> WallTime {
        NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    fn data() -> ChannelData {
        let mut d = ChannelData::new(".".into(), ChannelConfig::new("t"));
        for (id, mins) in [("a", 25), ("b", 25), ("news", 10)] {
            d.library.upsert(MediaItem::new(id, format!("{id}.mp4"), MediaKind::Program, Dur::mins(mins)));
        }
        d.playlists.push(Playlist::new("day").with_media(&["a", "b"]));
        d.playlists.push(Playlist::new("news").with_media(&["news"]));
        let t = |h| chrono::NaiveTime::from_hms_opt(h, 0, 0).unwrap();
        d.schedules = ScheduleSet {
            schedules: vec![Schedule {
                id: "s".into(),
                name: "s".into(),
                activate_at: None,
                enabled: true,
                week: WeekMap { default: Some("d".into()), ..Default::default() },
                exceptions: vec![],
                templates: vec![DayTemplate {
                    id: "d".into(),
                    name: "d".into(),
                    blocks: vec![
                        Block::new("day", Some(t(6)), BlockSource::Playlist { playlist: "day".into(), restart: false }),
                        Block::new("news", Some(t(7)), BlockSource::Playlist { playlist: "news".into(), restart: false }),
                    ],
                }],
            }],
        };
        d
    }

    /// Simulate airing: take items one by one with a given lateness.
    fn run(s: &mut Scheduler, d: &ChannelData, mut now: WallTime, n: usize, late_ms: Ms) -> Vec<(String, WallTime)> {
        let mut aired = vec![];
        for _ in 0..n {
            let Some(it) = s.take_next(d, now) else { break };
            s.on_start(&it);
            let end = add_ms(now, it.duration_ms + late_ms);
            s.replan_soft(d, &it, end);
            aired.push((it.media.clone(), now));
            now = end;
        }
        aired
    }

    #[test]
    fn fixed_block_stays_on_time_despite_lateness() {
        let d = data();
        let mut s = Scheduler::new(PlanState::default(), MemHistory::new());
        s.replan_hard(&d, at(6, 0));
        let aired = run(&mut s, &d, at(6, 0), 12, 5_000);
        let news = aired.iter().find(|(m, _)| m == "news").expect("news aired");
        // two 25-min items (+5 s each), the 10-min gap absorbs the delay
        assert!((news.1 - at(7, 0)).num_seconds().abs() <= 6, "news at {}", news.1);
    }

    #[test]
    fn queue_and_manual_mode() {
        let d = data();
        let mut s = Scheduler::new(PlanState::default(), MemHistory::new());
        s.replan_hard(&d, at(6, 0));
        let q = s.manual_item(&d, "news", at(6, 0)).unwrap();
        s.queue.push_back(q);
        assert_eq!(s.take_next(&d, at(6, 0)).unwrap().media, "news");
        s.manual = true;
        assert!(s.take_next(&d, at(6, 10)).is_none());
        s.manual = false;
        assert_eq!(s.take_next(&d, at(6, 10)).unwrap().media, "a");
    }

    #[test]
    fn early_item_for_fixed_block_waits_for_gap_fill() {
        let d = data();
        let mut s = Scheduler::new(PlanState::default(), MemHistory::new());
        s.replan_hard(&d, at(6, 0));
        // pretend everything before news failed instantly
        s.rundown.items.retain(|i| i.media == "news");
        let it = s.take_next(&d, at(6, 30)).unwrap();
        assert_ne!(it.media, "news", "must not start the 07:00 block at 06:30");
    }

    #[test]
    fn ad_break_and_standby() {
        let mut d = data();
        d.library.upsert(MediaItem::new("ad", "ad.mp4", MediaKind::Ad, Dur::secs(30)));
        d.rules.ad_breaks.push(lb_core::rules::AdBreak {
            id: "brk".into(),
            name: "b".into(),
            items: vec!["ad".into()],
            category: None,
            order: Default::default(),
            duration: None,
            max_spots: None,
            intro: None,
            outro: None,
            fill_category: None,
        });
        let mut s = Scheduler::new(PlanState::default(), MemHistory::new());
        let items = s.ad_break_items(&d, "brk", at(6, 0));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].origin, Origin::Manual);
        let sb = s.standby_item(&d, at(6, 0), 10_000);
        assert!(sb.is_slate());
    }
}

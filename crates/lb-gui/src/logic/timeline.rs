//! Timeline model: plan items → block segments for one day (minutes from
//! midnight), gaps, conflicts, the current block, as-run intervals; time
//! parsing for the block editor.

use chrono::{NaiveDate, NaiveTime, Timelike};

use lb_core::history::AsRunRecord;
use lb_core::media::MediaKind;
use lb_core::rundown::{Rundown, RundownItem};
use lb_core::{WallTime, diff_ms};

use crate::fmt::kind_id;

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: f32,
    pub duration: f32,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub block_id: Option<String>,
    pub gap: bool,
    pub conflict: bool,
    pub current: bool,
}

fn minutes(day: NaiveDate, t: WallTime) -> f32 {
    diff_ms(t, day.and_hms_opt(0, 0, 0).unwrap_or_default()) as f32 / 60_000.0
}

/// Block id from the plan's block key (`<date>:<block id>`).
pub fn block_id_of(it: &RundownItem) -> Option<String> {
    it.block_key.as_deref().map(|k| k.rsplit_once(':').map(|(_, b)| b).unwrap_or(k).to_string())
}

/// Dominant kind of a block: program if present, else the first item's kind.
fn dominant(items: &[&RundownItem]) -> &'static str {
    if items.iter().any(|i| i.kind == MediaKind::Program && !i.is_slate()) {
        "program"
    } else {
        items.iter().find(|i| !i.is_slate()).map(|i| kind_id(i.kind)).unwrap_or("filler")
    }
}

/// Group the plan into segments for `day` (items overlapping the day only).
/// Consecutive items of the same block form one segment; empty slots
/// (slate) become separate gap segments.
pub fn segments(plan: &Rundown, day: NaiveDate, now: Option<WallTime>) -> Vec<Segment> {
    let warn_at: Vec<f32> = plan.warnings.iter().filter_map(|w| w.at).map(|t| minutes(day, t)).collect();
    let now_m = now.map(|t| minutes(day, t));
    let mut out: Vec<Segment> = Vec::new();
    let mut group: Vec<&RundownItem> = Vec::new();
    let flush = |group: &mut Vec<&RundownItem>, out: &mut Vec<Segment>| {
        let (Some(first), Some(last)) = (group.first(), group.last()) else { return };
        let start = minutes(day, first.start);
        let end = minutes(day, last.end());
        let title = first.block_title.clone().filter(|t| !t.is_empty()).or_else(|| block_id_of(first)).unwrap_or_default();
        out.push(Segment {
            start,
            duration: (end - start).max(0.0),
            title,
            subtitle: format!("{} эл.", group.len()),
            kind: dominant(group).into(),
            block_id: block_id_of(first),
            gap: false,
            conflict: warn_at.iter().any(|&w| w >= start && w < end) && group.iter().any(|i| i.fixed_deviation_ms().is_some_and(|d| d != 0)),
            current: now_m.is_some_and(|n| n >= start && n < end),
        });
        group.clear();
    };
    for it in &plan.items {
        let (s, e) = (minutes(day, it.start), minutes(day, it.end()));
        if e <= 0.0 || s >= 1440.0 {
            continue;
        }
        if it.is_slate() {
            flush(&mut group, &mut out);
            out.push(Segment {
                start: s,
                duration: e - s,
                title: "Пустота".into(),
                subtitle: String::new(),
                kind: "gap".into(),
                block_id: block_id_of(it),
                gap: true,
                conflict: false,
                current: now_m.is_some_and(|n| n >= s && n < e),
            });
            continue;
        }
        if group.last().is_some_and(|l| l.block_key != it.block_key) {
            flush(&mut group, &mut out);
        }
        group.push(it);
    }
    flush(&mut group, &mut out);
    out
}

/// As-run records → actual-time segments (by block) for `day`.
pub fn actual(records: &[AsRunRecord], day: NaiveDate) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for r in records {
        let (s, e) = (minutes(day, r.started), minutes(day, r.ended));
        if e <= 0.0 || s >= 1440.0 {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.block_id == r.block && (s - (last.start + last.duration)).abs() < 1.0 => {
                last.duration = e - last.start;
            }
            _ => out.push(Segment {
                start: s,
                duration: e - s,
                title: String::new(),
                subtitle: String::new(),
                kind: kind_id(r.kind).into(),
                block_id: r.block.clone(),
                gap: false,
                conflict: false,
                current: false,
            }),
        }
    }
    out
}

/// Parse `HH:MM` or `HH:MM:SS`; empty = `Ok(None)` (follow block).
pub fn parse_time(s: &str) -> Result<Option<NaiveTime>, &'static str> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M"))
        .map(Some)
        .map_err(|_| "Формат ЧЧ:ММ или ЧЧ:ММ:СС")
}

/// Minutes from midnight → time, snapped to `snap` minutes, wrapped to the day.
pub fn time_of(minutes: f32, snap: f32) -> NaiveTime {
    let m = if snap > 0.0 { (minutes / snap).round() * snap } else { minutes };
    let secs = ((m * 60.0).round() as i64).rem_euclid(86_400) as u32;
    NaiveTime::from_num_seconds_from_midnight_opt(secs, 0).unwrap_or_default()
}

pub fn minutes_of(t: NaiveTime) -> f32 {
    t.num_seconds_from_midnight() as f32 / 60.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::rundown::{Origin, RundownItem};

    fn item(block: &str, h: u32, m: u32, secs: i64, slate: bool) -> RundownItem {
        let day = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let mut it: RundownItem = serde_json::from_value(serde_json::json!({
            "uid": format!("{block}-{h}-{m}"),
            "media": if slate { "" } else { "x" },
            "title": "t",
            "kind": "program",
            "origin": if slate { "slate" } else { "main" },
            "start": day.and_hms_opt(h, m, 0).unwrap(),
            "duration_ms": secs * 1000,
        }))
        .unwrap();
        it.block_key = Some(format!("2026-10-21:{block}"));
        it.block_title = Some(block.to_uppercase());
        if slate {
            it.origin = Origin::Slate;
        }
        it
    }

    #[test]
    fn groups_blocks_and_gaps() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let plan = Rundown {
            items: vec![item("a", 9, 0, 600, false), item("a", 9, 10, 600, false), item("a", 9, 20, 300, true), item("b", 9, 25, 300, false)],
            ..Default::default()
        };
        let now = day.and_hms_opt(9, 12, 0);
        let s = segments(&plan, day, now);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].start, s[0].duration, s[0].title.as_str()), (540.0, 20.0, "A"));
        assert!(s[0].current);
        assert!(s[1].gap);
        assert_eq!(s[2].block_id.as_deref(), Some("b"));
    }

    #[test]
    fn time_parsing_and_snapping() {
        assert_eq!(parse_time(""), Ok(None));
        assert_eq!(parse_time("7:05"), Ok(NaiveTime::from_hms_opt(7, 5, 0)));
        assert!(parse_time("25:00").is_err());
        assert_eq!(time_of(62.4, 5.0), NaiveTime::from_hms_opt(1, 0, 0).unwrap());
        assert_eq!(time_of(-10.0, 5.0), NaiveTime::from_hms_opt(23, 50, 0).unwrap());
    }
}

//! Statistics and reports from the as-run log.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use lb_core::Ms;
use lb_core::history::{AsRunRecord, AsRunStatus};
use lb_core::media::MediaKind;

use crate::persist::asrun;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MediaStats {
    pub media: String,
    pub title: String,
    pub kind: Option<MediaKind>,
    pub plays: u32,
    pub completed: u32,
    pub interrupted: u32,
    pub skipped: u32,
    pub airtime_ms: Ms,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub items: usize,
    pub airtime_ms: Ms,
    pub skipped: usize,
    pub interrupted: usize,
    /// Average |actual − planned| start deviation.
    pub avg_deviation_ms: Ms,
    pub max_deviation_ms: Ms,
    pub by_media: Vec<MediaStats>,
    pub ads: Vec<MediaStats>,
    pub promos: Vec<MediaStats>,
}

/// Aggregate records.
pub fn aggregate(recs: &[AsRunRecord]) -> Report {
    let mut by: BTreeMap<String, MediaStats> = BTreeMap::new();
    let mut r = Report { items: recs.len(), ..Default::default() };
    let mut dev_sum: i64 = 0;
    let mut dev_n: i64 = 0;
    for rec in recs {
        let s = by.entry(rec.media.clone()).or_insert_with(|| MediaStats {
            media: rec.media.clone(),
            title: rec.title.clone(),
            kind: Some(rec.kind),
            ..Default::default()
        });
        match rec.status {
            AsRunStatus::Completed | AsRunStatus::Truncated => {
                s.plays += 1;
                s.completed += 1;
            }
            AsRunStatus::Interrupted => {
                s.plays += 1;
                s.interrupted += 1;
                r.interrupted += 1;
            }
            AsRunStatus::Skipped => {
                s.skipped += 1;
                r.skipped += 1;
            }
        }
        if rec.status != AsRunStatus::Skipped {
            s.airtime_ms += rec.played_ms;
            r.airtime_ms += rec.played_ms;
            let d = rec.deviation_ms().abs();
            dev_sum += d;
            dev_n += 1;
            r.max_deviation_ms = r.max_deviation_ms.max(d);
        }
    }
    r.avg_deviation_ms = if dev_n > 0 { dev_sum / dev_n } else { 0 };
    let mut all: Vec<MediaStats> = by.into_values().filter(|s| !s.media.is_empty()).collect();
    all.sort_by(|a, b| b.plays.cmp(&a.plays).then(a.media.cmp(&b.media)));
    r.ads = all.iter().filter(|s| s.kind == Some(MediaKind::Ad)).cloned().collect();
    r.promos = all.iter().filter(|s| s.kind == Some(MediaKind::Promo)).cloned().collect();
    r.by_media = all;
    r
}

/// Report for a date range of a channel's as-run directory.
pub fn report(asrun_dir: &Path, from: NaiveDate, to: NaiveDate) -> Report {
    let recs = asrun::read_range(asrun_dir, from, to);
    let mut r = aggregate(&recs);
    r.from = Some(from);
    r.to = Some(to);
    r
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// As-run records as CSV (planned vs actual).
pub fn asrun_csv(recs: &[AsRunRecord]) -> String {
    let mut s = String::from("planned_start,started,ended,deviation_s,media,title,kind,block,rule,status,planned_s,played_s,note\n");
    for r in recs {
        s.push_str(&format!(
            "{},{},{},{:.1},{},{},{:?},{},{},{:?},{:.1},{:.1},{}\n",
            r.planned_start,
            r.started,
            r.ended,
            r.deviation_ms() as f64 / 1000.0,
            csv_escape(&r.media),
            csv_escape(&r.title),
            r.kind,
            csv_escape(r.block.as_deref().unwrap_or("")),
            csv_escape(r.rule.as_deref().unwrap_or("")),
            r.status,
            r.planned_duration_ms as f64 / 1000.0,
            r.played_ms as f64 / 1000.0,
            csv_escape(&r.note)
        ));
    }
    s
}

/// Per-media statistics as CSV.
pub fn stats_csv(stats: &[MediaStats]) -> String {
    let mut s = String::from("media,title,kind,plays,completed,interrupted,skipped,airtime_s\n");
    for m in stats {
        s.push_str(&format!(
            "{},{},{:?},{},{},{},{},{:.0}\n",
            csv_escape(&m.media),
            csv_escape(&m.title),
            m.kind.unwrap_or_default(),
            m.plays,
            m.completed,
            m.interrupted,
            m.skipped,
            m.airtime_ms as f64 / 1000.0
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(media: &str, kind: MediaKind, status: AsRunStatus, dev_s: i64) -> AsRunRecord {
        let t = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(10, 0, 0).unwrap();
        AsRunRecord {
            channel: "c".into(),
            uid: "u".into(),
            media: media.into(),
            title: format!("T, {media}"),
            kind,
            category: None,
            block: None,
            rule: None,
            planned_start: t,
            started: t + chrono::Duration::seconds(dev_s),
            ended: t + chrono::Duration::seconds(dev_s + 30),
            planned_duration_ms: 30_000,
            played_ms: 30_000,
            status,
            note: String::new(),
        }
    }

    #[test]
    fn aggregates_ads_and_deviation() {
        let recs = vec![
            rec("ad1", MediaKind::Ad, AsRunStatus::Completed, 2),
            rec("ad1", MediaKind::Ad, AsRunStatus::Completed, -4),
            rec("p", MediaKind::Program, AsRunStatus::Skipped, 0),
        ];
        let r = aggregate(&recs);
        assert_eq!(r.ads.len(), 1);
        assert_eq!(r.ads[0].plays, 2);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.avg_deviation_ms, 3_000);
        assert_eq!(r.max_deviation_ms, 4_000);
        let csv = asrun_csv(&recs);
        assert!(csv.contains("\"T, ad1\""));
        assert_eq!(csv.lines().count(), 4);
    }
}

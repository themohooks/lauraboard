//! As-run log: one JSON line per aired (or skipped) item, one file per day.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use lb_core::history::{AsRunRecord, MemHistory};

use crate::error::Result;

pub fn file_for(dir: &Path, date: NaiveDate) -> PathBuf {
    dir.join(format!("{date}.jsonl"))
}

/// Append a record to the day file of its start time.
pub fn append(dir: &Path, rec: &AsRunRecord) -> Result<()> {
    fs::create_dir_all(dir)?;
    let mut f = OpenOptions::new().create(true).append(true).open(file_for(dir, rec.started.date()))?;
    let line = serde_json::to_string(rec).map_err(|e| crate::error::EngineError::Other(e.to_string()))?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Read records for `[from, to]` (inclusive dates). Corrupt lines are skipped.
pub fn read_range(dir: &Path, from: NaiveDate, to: NaiveDate) -> Vec<AsRunRecord> {
    let mut out = Vec::new();
    let mut d = from;
    while d <= to {
        if let Ok(f) = fs::File::open(file_for(dir, d)) {
            for line in BufReader::new(f).lines().map_while(|l| l.ok()) {
                if let Ok(r) = serde_json::from_str::<AsRunRecord>(&line) {
                    out.push(r);
                }
            }
        }
        d = d.succ_opt().unwrap_or(d);
        if d == NaiveDate::MAX {
            break;
        }
    }
    out
}

/// Build planner history from the last `days` of logs.
pub fn load_history(dir: &Path, today: NaiveDate, days: i64) -> MemHistory {
    let recs = read_range(dir, today - chrono::Duration::days(days.max(1)), today);
    MemHistory::from_records(&recs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::history::{AsRunStatus, PlayHistory};
    use lb_core::media::MediaKind;

    fn rec(media: &str, d: u32) -> AsRunRecord {
        let t = NaiveDate::from_ymd_opt(2026, 10, d).unwrap().and_hms_opt(10, 0, 0).unwrap();
        AsRunRecord {
            channel: "main".into(),
            uid: "u".into(),
            media: media.into(),
            title: media.into(),
            kind: MediaKind::Ad,
            category: None,
            block: None,
            rule: None,
            planned_start: t,
            started: t,
            ended: t,
            planned_duration_ms: 1000,
            played_ms: 1000,
            status: AsRunStatus::Completed,
            note: String::new(),
        }
    }

    #[test]
    fn append_and_read() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), &rec("a", 20)).unwrap();
        append(dir.path(), &rec("b", 21)).unwrap();
        std::fs::write(file_for(dir.path(), NaiveDate::from_ymd_opt(2026, 10, 22).unwrap()), "garbage\n").unwrap();
        let d = |x| NaiveDate::from_ymd_opt(2026, 10, x).unwrap();
        assert_eq!(read_range(dir.path(), d(20), d(22)).len(), 2);
        let h = load_history(dir.path(), d(21), 1);
        assert_eq!(h.count_on("a", d(20)), 1);
    }
}

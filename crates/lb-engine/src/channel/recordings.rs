//! Recordings on disk: the file being written now, totals, and clean-up
//! of old files by age and by the total size of the channel's recordings.
//!
//! Only files named like the recorder names them are touched
//! (`<prefix>-YYYYMMDD-HHMMSS.<mp4|mkv|ts>`); the newest one is never
//! deleted (it is being written).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use lb_core::config::RecordConfig;

/// How often the directory is looked at for the status.
const SCAN_EVERY: Duration = Duration::from_secs(5);
/// How often old recordings are deleted.
const CLEAN_EVERY: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RecordingStatus {
    /// Directory (resolved).
    pub dir: String,
    /// File being written (name only).
    pub file: Option<String>,
    pub file_bytes: u64,
    /// All recordings of the channel in the directory.
    pub files: usize,
    pub total_bytes: u64,
    /// Free space on the disk, when known.
    #[serde(default)]
    pub free_bytes: Option<u64>,
}

/// A recording file.
#[derive(Debug, Clone)]
pub struct RecFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: SystemTime,
}

/// `name` is `<prefix>-YYYYMMDD-HHMMSS.<ext>` with a recorder extension.
fn is_recording(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix).and_then(|r| r.strip_prefix('-')) else { return false };
    let Some((stamp, ext)) = rest.rsplit_once('.') else { return false };
    let ok_stamp = stamp.len() == 15 && stamp.as_bytes()[8] == b'-' && stamp.bytes().enumerate().all(|(i, b)| i == 8 || b.is_ascii_digit());
    ok_stamp && matches!(ext, "mp4" | "mkv" | "ts")
}

/// Recordings of `prefix` in `dir`, oldest first.
pub fn list(dir: &Path, prefix: &str) -> Vec<RecFile> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<RecFile> = rd
        .flatten()
        .filter(|e| is_recording(&e.file_name().to_string_lossy(), prefix))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| RecFile { path: e.path(), bytes: m.len(), modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH) })
        })
        .collect();
    // names sort by time; mtime breaks ties between equal names in theory only
    v.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()).then(a.modified.cmp(&b.modified)));
    v
}

/// Files to delete: older than `keep_days`, then the oldest while the total
/// exceeds `max_total_gb`. The newest file is always kept.
pub fn to_delete(files: &[RecFile], rec: &RecordConfig, now: SystemTime) -> Vec<PathBuf> {
    let Some((_, older)) = files.split_last() else { return vec![] };
    let mut out: Vec<PathBuf> = Vec::new();
    let mut total: u64 = files.iter().map(|f| f.bytes).sum();
    let max_age = (rec.keep_days > 0).then(|| Duration::from_secs(rec.keep_days as u64 * 86_400));
    let max_total = (rec.max_total_gb > 0.0).then_some((rec.max_total_gb * 1e9) as u64);
    for f in older {
        let too_old = max_age.is_some_and(|age| now.duration_since(f.modified).is_ok_and(|d| d > age));
        let too_big = max_total.is_some_and(|m| total > m);
        if too_old || too_big {
            total = total.saturating_sub(f.bytes);
            out.push(f.path.clone());
        }
    }
    out
}

/// Keeps the recording status fresh and the directory tidy.
#[derive(Default)]
pub struct Recordings {
    /// (directory, file prefix, settings) while recording is on.
    target: Option<(PathBuf, String, RecordConfig)>,
    status: Option<RecordingStatus>,
    scanned: Option<Instant>,
    cleaned: Option<Instant>,
}

impl Recordings {
    /// Recording settings in effect (`None`: recording is off).
    pub fn set(&mut self, target: Option<(PathBuf, String, RecordConfig)>) {
        if self.target.as_ref().map(|t| (&t.0, &t.1, &t.2)) != target.as_ref().map(|t| (&t.0, &t.1, &t.2)) {
            self.scanned = None;
            self.cleaned = None;
        }
        if target.is_none() {
            self.status = None;
        }
        self.target = target;
    }

    pub fn status(&self) -> Option<RecordingStatus> {
        self.status.clone()
    }

    /// Rescan / clean up when due; deleted files are returned for logging.
    pub fn tick(&mut self) -> Vec<PathBuf> {
        let Some((dir, prefix, rec)) = &self.target else { return vec![] };
        let mut deleted = Vec::new();
        if self.cleaned.is_none_or(|t| t.elapsed() >= CLEAN_EVERY) {
            self.cleaned = Some(Instant::now());
            for p in to_delete(&list(dir, prefix), rec, SystemTime::now()) {
                match std::fs::remove_file(&p) {
                    Ok(()) => deleted.push(p),
                    Err(e) => tracing::warn!("could not delete old recording {}: {e}", p.display()),
                }
            }
            self.scanned = None;
        }
        if self.scanned.is_none_or(|t| t.elapsed() >= SCAN_EVERY) {
            self.scanned = Some(Instant::now());
            let files = list(dir, prefix);
            let last = files.last();
            self.status = Some(RecordingStatus {
                dir: dir.display().to_string(),
                file: last.and_then(|f| f.path.file_name()).map(|n| n.to_string_lossy().into_owned()),
                file_bytes: last.map(|f| f.bytes).unwrap_or(0),
                files: files.len(),
                total_bytes: files.iter().map(|f| f.bytes).sum(),
                free_bytes: crate::system::free_space(dir),
            });
        }
        deleted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(name: &str, gb: f64, days_ago: u64) -> RecFile {
        RecFile {
            path: PathBuf::from(name),
            bytes: (gb * 1e9) as u64,
            modified: SystemTime::now() - Duration::from_secs(days_ago * 86_400 + 60),
        }
    }

    #[test]
    fn names() {
        assert!(is_recording("main-20261024-120000.mp4", "main"));
        assert!(is_recording("main-20261024-120000.ts", "main"));
        assert!(!is_recording("main-20261024-120000.mov", "main"));
        assert!(!is_recording("main2-20261024-120000.mp4", "main"));
        assert!(!is_recording("main-notes.mp4", "main"));
    }

    #[test]
    fn deletes_old_and_oversize_but_never_the_newest() {
        let files = vec![f("a", 1.0, 10), f("b", 1.0, 5), f("c", 1.0, 1), f("d", 1.0, 40)];
        let rec = RecordConfig { keep_days: 7, ..Default::default() };
        assert_eq!(to_delete(&files, &rec, SystemTime::now()), vec![PathBuf::from("a")]);
        let rec = RecordConfig { max_total_gb: 2.5, ..Default::default() };
        assert_eq!(to_delete(&files, &rec, SystemTime::now()), vec![PathBuf::from("a"), PathBuf::from("b")]);
        assert!(to_delete(&files, &RecordConfig::default(), SystemTime::now()).is_empty());
    }
}

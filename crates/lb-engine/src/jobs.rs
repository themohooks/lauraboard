//! Background jobs with progress (media copying, channel export / import).
//! Clients poll the list; a job can be cancelled while it runs and
//! dismissed once it has finished.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: u64,
    /// `import_media`, `export_channel`, `import_channel` …
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub state: JobState,
    /// 0…1.
    pub progress: f32,
    /// Current step or the error.
    pub message: String,
    #[serde(default)]
    pub result: Option<Value>,
    /// Estimated seconds left (running jobs, once the pace is known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_secs: Option<u64>,
    /// Transfer speed, bytes per second (downloads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
}

struct Entry {
    info: JobInfo,
    cancel: Arc<AtomicBool>,
    /// When the progress first moved, and from where: the pace since then
    /// gives the time left (setup before the first step does not count).
    pace_from: Option<(Instant, f32)>,
}

/// Registry of jobs (shared by the engine and its API).
#[derive(Clone, Default)]
pub struct Jobs {
    list: Arc<Mutex<Vec<Entry>>>,
    next: Arc<AtomicU64>,
}

/// Handle given to a running job to report progress.
#[derive(Clone)]
pub struct JobCtl {
    id: u64,
    jobs: Jobs,
    cancel: Arc<AtomicBool>,
}

/// Finished jobs kept in the list (oldest are forgotten first).
const KEEP_FINISHED: usize = 20;

impl Jobs {
    fn with<R>(&self, f: impl FnOnce(&mut Vec<Entry>) -> R) -> R {
        f(&mut self.list.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn list(&self) -> Vec<JobInfo> {
        self.with(|l| l.iter().map(|e| e.info.clone()).collect())
    }

    pub fn get(&self, id: u64) -> Option<JobInfo> {
        self.with(|l| l.iter().find(|e| e.info.id == id).map(|e| e.info.clone()))
    }

    /// Start `work` on the tokio runtime; returns the job id.
    pub fn spawn<F, Fut>(&self, kind: &str, title: &str, channel: Option<&str>, work: F) -> u64
    where
        F: FnOnce(JobCtl) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let info = JobInfo {
            id,
            kind: kind.into(),
            title: title.into(),
            channel: channel.map(str::to_string),
            state: JobState::Running,
            progress: 0.0,
            message: String::new(),
            result: None,
            eta_secs: None,
            speed: None,
        };
        self.with(|l| {
            l.push(Entry { info, cancel: cancel.clone(), pace_from: None });
            prune(l);
        });
        let ctl = JobCtl { id, jobs: self.clone(), cancel };
        tokio::spawn(async move {
            let jobs = ctl.jobs.clone();
            let cancelled = ctl.cancel.clone();
            let outcome = work(ctl).await;
            jobs.update(id, |j| {
                j.eta_secs = None;
                j.speed = None;
            });
            jobs.update(id, |j| match outcome {
                _ if cancelled.load(Ordering::Relaxed) => {
                    j.state = JobState::Cancelled;
                    j.message = "cancelled".into();
                }
                Ok(v) => {
                    j.state = JobState::Done;
                    j.progress = 1.0;
                    j.result = Some(v);
                }
                Err(e) => {
                    tracing::warn!(job = id, "job failed: {e}");
                    j.state = JobState::Failed;
                    j.message = e;
                }
            });
        });
        id
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut JobInfo)) {
        self.with(|l| {
            if let Some(e) = l.iter_mut().find(|e| e.info.id == id) {
                f(&mut e.info);
            }
        });
    }

    /// Cancel a running job, or forget a finished one.
    pub fn cancel_or_dismiss(&self, id: u64) -> bool {
        self.with(|l| {
            let Some(pos) = l.iter().position(|e| e.info.id == id) else { return false };
            if l[pos].info.state == JobState::Running {
                l[pos].cancel.store(true, Ordering::Relaxed);
                l[pos].info.message = "cancelling…".into();
            } else {
                l.remove(pos);
            }
            true
        })
    }
}

/// Seconds left when `done` of the work took `elapsed` seconds; unknown
/// while there is too little to judge by.
fn eta(elapsed: f32, done: f32, left: f32) -> Option<u64> {
    (elapsed >= 2.0 && done >= 0.01).then(|| (elapsed * left / done).round() as u64)
}

fn prune(l: &mut Vec<Entry>) {
    while l.iter().filter(|e| e.info.state != JobState::Running).count() > KEEP_FINISHED {
        let Some(pos) = l.iter().position(|e| e.info.state != JobState::Running) else { break };
        l.remove(pos);
    }
}

impl JobCtl {
    pub fn progress(&self, fraction: f32, message: impl Into<String>) {
        let message = message.into();
        let fraction = fraction.clamp(0.0, 1.0);
        let now = Instant::now();
        self.jobs.with(|l| {
            let Some(e) = l.iter_mut().find(|e| e.info.id == self.id) else { return };
            e.info.progress = fraction;
            e.info.message = message;
            match e.pace_from {
                Some((_, from)) if fraction < from => e.pace_from = Some((now, fraction)),
                Some((at, from)) => e.info.eta_secs = eta(now.duration_since(at).as_secs_f32(), fraction - from, 1.0 - fraction),
                None if fraction > 0.0 => e.pace_from = Some((now, fraction)),
                None => {}
            }
        });
    }

    /// Current transfer speed (bytes per second; `None` clears it).
    pub fn speed(&self, bytes_per_sec: Option<f64>) {
        self.jobs.update(self.id, |j| j.speed = bytes_per_sec.filter(|s| s.is_finite() && *s > 0.0));
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// `Err("cancelled")` once the job was cancelled (use with `?`).
    pub fn check(&self) -> Result<(), String> {
        if self.cancelled() { Err("cancelled".into()) } else { Ok(()) }
    }
}

/// Copy a file in chunks, reporting the bytes done (for progress) and
/// stopping when the job is cancelled.
pub fn copy_with_progress(
    from: &std::path::Path,
    to: &std::path::Path,
    ctl: &JobCtl,
    mut on_bytes: impl FnMut(u64),
) -> Result<u64, String> {
    use std::io::{Read, Write};
    let mut src = std::fs::File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut dst = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        if ctl.cancelled() {
            drop(dst);
            let _ = std::fs::remove_file(to);
            return Err("cancelled".into());
        }
        let n = src.read(&mut buf).map_err(|e| format!("{}: {e}", from.display()))?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", to.display()))?;
        total += n as u64;
        on_bytes(n as u64);
    }
    dst.flush().map_err(|e| e.to_string())?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_the_time_left() {
        assert_eq!(eta(1.0, 0.5, 0.5), None, "too early");
        assert_eq!(eta(10.0, 0.005, 0.9), None, "too little done");
        assert_eq!(eta(10.0, 0.25, 0.5), Some(20));
    }

    #[tokio::test]
    async fn job_reports_and_finishes() {
        let jobs = Jobs::default();
        let id = jobs.spawn("test", "Test", None, |ctl| async move {
            ctl.progress(0.5, "half");
            Ok(serde_json::json!({ "n": 1 }))
        });
        for _ in 0..50 {
            if jobs.get(id).unwrap().state != JobState::Running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let j = jobs.get(id).unwrap();
        assert_eq!(j.state, JobState::Done);
        assert_eq!(j.result.unwrap()["n"], 1);
        assert!(jobs.cancel_or_dismiss(id));
        assert!(jobs.get(id).is_none());
    }

    #[tokio::test]
    async fn cancelled_job() {
        let jobs = Jobs::default();
        let id = jobs.spawn("test", "Test", None, |ctl| async move {
            while !ctl.cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            Err("cancelled".into())
        });
        jobs.cancel_or_dismiss(id);
        for _ in 0..100 {
            if jobs.get(id).unwrap().state != JobState::Running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(jobs.get(id).unwrap().state, JobState::Cancelled);
    }
}

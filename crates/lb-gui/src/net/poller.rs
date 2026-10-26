//! Periodic refresh of live data for the selected channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::api::Api;
use super::{Frame, Msg, Wake};
use crate::model::StatusView;

/// Shared poller settings.
#[derive(Default)]
pub struct PollTarget {
    pub channel: Option<String>,
    pub preview: bool,
    pub revision: u64,
}

pub struct Poller {
    pub target: Arc<Mutex<PollTarget>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn decode_jpeg(bytes: &[u8]) -> Option<Frame> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).ok()?.to_rgba8();
    Some(Frame { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

/// Interval between preview requests (the engine produces 15 frames/s).
const PREVIEW_EVERY: Duration = Duration::from_millis(66);

/// Separate loop for the live picture: much faster than the status poll,
/// and an unchanged frame is neither decoded nor sent to the window.
fn spawn_preview(api: Api, tx: std::sync::mpsc::Sender<Msg>, wake: Wake, target: Arc<Mutex<PollTarget>>, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut last: Vec<u8> = Vec::new();
        let mut last_channel = None;
        while !stop.load(Ordering::Relaxed) {
            let started = Instant::now();
            let (channel, wanted, revision) = {
                let g = target.lock().unwrap_or_else(|e| e.into_inner());
                (g.channel.clone(), g.preview, g.revision)
            };
            let mut pause = PREVIEW_EVERY;
            if Some((channel.clone(), revision)) != last_channel {
                last.clear();
                last_channel = Some((channel.clone(), revision));
            }
            match channel.filter(|_| wanted) {
                Some(ch) => match api.get_bytes(&format!("/channels/{ch}/preview.jpg")) {
                    Ok(bytes) if bytes != last => {
                        if let Some(img) = decode_jpeg(&bytes) {
                            let _ = tx.send(Msg::ForChannel(ch.clone(), revision, Box::new(Msg::Preview(img))));
                            wake();
                        }
                        last = bytes;
                    }
                    Ok(_) => {}
                    // no frame yet / offline: back off
                    Err(_) => pause = Duration::from_secs(1),
                },
                None => pause = Duration::from_millis(300),
            }
            std::thread::sleep(pause.saturating_sub(started.elapsed()));
        }
    });
}

impl Poller {
    pub fn start(api: Api, tx: std::sync::mpsc::Sender<Msg>, wake: Wake) -> Self {
        let target = Arc::new(Mutex::new(PollTarget { channel: None, preview: true, revision: 0 }));
        let stop = Arc::new(AtomicBool::new(false));
        spawn_preview(api.clone(), tx.clone(), wake.clone(), target.clone(), stop.clone());
        let (t2, s2) = (target.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut last_rundown = Instant::now() - Duration::from_secs(60);
            let mut online = None;
            let mut last_jobs = String::new();
            // the polled channel answers with an error while the server is up
            let mut channel_lost = false;
            while !s2.load(Ordering::Relaxed) {
                let (channel, revision) = {
                    let g = t2.lock().unwrap_or_else(|e| e.into_inner());
                    (g.channel.clone(), g.revision)
                };
                // no channel (none in the project yet, or the last one was
                // deleted): the job list alone tells whether the server is up
                let status = channel.as_ref().map(|ch| api.get::<StatusView>(&format!("/channels/{ch}/status")));
                let jobs = match &status {
                    Some(Err(_)) | None => Some(api.get_bytes("/jobs")),
                    Some(Ok(_)) => None,
                };
                let lost = matches!((&status, &jobs), (Some(Err(_)), Some(Ok(_))));
                if lost && !channel_lost {
                    // deleted or renamed elsewhere: re-read the channel list
                    let _ = tx.send(Msg::Apply(Box::new(|c| c.load_channels())));
                }
                channel_lost = lost;
                let ok = matches!(status, Some(Ok(_))) || matches!(jobs, Some(Ok(_)));
                if online != Some(ok) {
                    online = Some(ok);
                    let _ = tx.send(Msg::Connection(ok));
                }
                if let Some(Ok(s)) = status {
                    if let Some(ch) = &channel {
                        let _ = tx.send(Msg::ForChannel(ch.clone(), revision, Box::new(Msg::Status(Box::new(s)))));
                    }
                }
                let jobs = match jobs {
                    Some(r) => r,
                    None if ok => api.get_bytes("/jobs"),
                    None => Err(String::new()),
                };
                if ok && let Ok(body) = jobs {
                    let text = String::from_utf8_lossy(&body).into_owned();
                    if text != last_jobs
                        && let Ok(jobs) = serde_json::from_str(&text)
                    {
                        last_jobs = text;
                        let _ = tx.send(Msg::Jobs(jobs));
                    }
                }
                if let Some(ch) = channel.filter(|_| ok && !lost)
                    && last_rundown.elapsed() >= Duration::from_secs(2)
                {
                    last_rundown = Instant::now();
                    if let Ok(r) = api.get(&format!("/channels/{ch}/rundown?limit=300")) {
                        let _ = tx.send(Msg::ForChannel(ch, revision, Box::new(Msg::Rundown(r))));
                    }
                }
                wake();
                std::thread::sleep(Duration::from_millis(if ok { 500 } else { 2000 }));
            }
        });
        Poller { target, stop }
    }

    pub fn channel(&self) -> Option<String> {
        self.target.lock().ok().and_then(|g| g.channel.clone())
    }

    pub fn revision(&self) -> u64 {
        self.target.lock().unwrap_or_else(|e| e.into_inner()).revision
    }

    pub fn set_channel(&self, ch: &str) {
        if let Ok(mut g) = self.target.lock() {
            if g.channel.as_deref() != Some(ch) { g.revision += 1; }
            g.channel = Some(ch.to_string());
        }
    }

    /// Stop polling channel data (no channel left).
    pub fn clear_channel(&self) {
        if let Ok(mut g) = self.target.lock() {
            g.revision += 1;
            g.channel = None;
        }
    }
}

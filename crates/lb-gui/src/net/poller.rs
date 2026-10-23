//! Periodic refresh of live data for the selected channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Frame, Msg, Wake};
use super::api::Api;
use crate::model::StatusView;

/// Shared poller settings.
#[derive(Default)]
pub struct PollTarget {
    pub channel: Option<String>,
    pub preview: bool,
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
        while !stop.load(Ordering::Relaxed) {
            let started = Instant::now();
            let (channel, wanted) = {
                let g = target.lock().unwrap_or_else(|e| e.into_inner());
                (g.channel.clone(), g.preview)
            };
            let mut pause = PREVIEW_EVERY;
            match channel.filter(|_| wanted) {
                Some(ch) => match api.get_bytes(&format!("/channels/{ch}/preview.jpg")) {
                    Ok(bytes) if bytes != last => {
                        if let Some(img) = decode_jpeg(&bytes) {
                            let _ = tx.send(Msg::Preview(img));
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
        let target = Arc::new(Mutex::new(PollTarget { channel: None, preview: true }));
        let stop = Arc::new(AtomicBool::new(false));
        spawn_preview(api.clone(), tx.clone(), wake.clone(), target.clone(), stop.clone());
        let (t2, s2) = (target.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut last_rundown = Instant::now() - Duration::from_secs(60);
            let mut online = None;
            while !s2.load(Ordering::Relaxed) {
                let channel = t2.lock().unwrap_or_else(|e| e.into_inner()).channel.clone();
                let Some(ch) = channel else {
                    std::thread::sleep(Duration::from_millis(300));
                    continue;
                };
                let status = api.get::<StatusView>(&format!("/channels/{ch}/status"));
                let ok = status.is_ok();
                if online != Some(ok) {
                    online = Some(ok);
                    let _ = tx.send(Msg::Connection(ok));
                }
                if let Ok(s) = status {
                    let _ = tx.send(Msg::Status(Box::new(s)));
                }
                if ok && last_rundown.elapsed() >= Duration::from_secs(2) {
                    last_rundown = Instant::now();
                    if let Ok(r) = api.get(&format!("/channels/{ch}/rundown?limit=300")) {
                        let _ = tx.send(Msg::Rundown(r));
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

    pub fn set_channel(&self, ch: &str) {
        if let Ok(mut g) = self.target.lock() {
            g.channel = Some(ch.to_string());
        }
    }
}

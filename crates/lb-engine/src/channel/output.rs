//! Encoder supervision: start, health check, restart with back-off.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use lb_core::config::FfmpegConfig;
use lb_core::project::{ChannelData, paths};
use lb_media::args::encoder::{EncodeJob, GfxInput, PreviewOut, encoder_args};
use lb_media::args::record::{RecordJob, recorder_args};
use lb_media::process::Encoder;
use lb_media::process::encoder::PreviewFrame;
use lb_media::progress::EncoderStats;

use super::playout::SharedStdin;
use super::recorder::RecorderSup;
use super::recordings::Recordings;
use super::status::OutputStatus;

const BACKOFF: &[u64] = &[1, 2, 5, 10, 20, 30];
/// A failing network output reports again on every retry (5 s); one not
/// heard of for this long is considered back.
const PROBLEM_FRESH: Duration = Duration::from_secs(15);

pub struct Output {
    pub stdin: SharedStdin,
    encoder: Option<Encoder>,
    codec: Option<String>,
    restarts: u32,
    failures_in_row: usize,
    retry_at: Option<Instant>,
    last_error: Option<String>,
    targets: Vec<String>,
    /// Enabled network outputs: (name, URL).
    network: Vec<(String, String)>,
    /// Latest preview frame (kept across encoder restarts).
    pub preview: tokio::sync::watch::Sender<PreviewFrame>,
    recordings: Recordings,
    recorder: RecorderSup,
}

impl Output {
    pub fn new(preview: tokio::sync::watch::Sender<PreviewFrame>) -> Self {
        Output {
            stdin: Arc::new(Mutex::new(None)),
            encoder: None,
            codec: None,
            restarts: 0,
            failures_in_row: 0,
            retry_at: None,
            last_error: None,
            targets: vec![],
            network: vec![],
            preview,
            recordings: Recordings::default(),
            recorder: RecorderSup::default(),
        }
    }

    pub fn running(&self) -> bool {
        self.encoder.is_some()
    }

    /// Start the encoder. `gfx` (port, consumed-frames counter) enables the
    /// graphics overlay input; the counter follows the encoder's progress.
    pub async fn start(
        &mut self,
        ff: &FfmpegConfig,
        data: &ChannelData,
        root: &Path,
        gfx: Option<(u16, Arc<AtomicU64>)>,
    ) -> Result<(), String> {
        let gfx_port = gfx.as_ref().map(|g| g.0);
        let cfg = &data.config;
        if self.codec.is_none() {
            self.codec = Some(lb_media::hw::resolve_codec(&ff.ffmpeg, &cfg.encoder.video_codec).await);
        }
        let codec = self.codec.clone().unwrap_or_else(|| "libx264".into());
        // the recorder listens first: the encoder connects to it at start
        self.apply_record(ff, data).await;
        let record_feed = Some(self.recorder.port());
        for o in cfg.outputs.iter().filter(|o| o.enabled && !lb_core::media::is_url(&o.url)) {
            if let Some(parent) = paths::resolve(root, &o.url).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let outputs: Vec<_> = cfg
            .outputs
            .iter()
            .map(|o| {
                let mut o = o.clone();
                if !lb_core::media::is_url(&o.url) {
                    o.url = paths::resolve_str(root, &o.url);
                }
                o
            })
            .collect();
        self.targets = outputs.iter().filter(|o| o.enabled).map(|o| o.name.clone()).collect();
        self.network =
            outputs.iter().filter(|o| o.enabled && lb_core::media::is_url(&o.url)).map(|o| (o.name.clone(), o.url.clone())).collect();
        let job = EncodeJob {
            channel: &cfg.id,
            video: &cfg.video,
            audio: &cfg.audio,
            encoder: &cfg.encoder,
            video_codec: &codec,
            outputs: &outputs,
            record_feed,
            // overlay at the video rate: anything slower makes motion stutter
            gfx: gfx_port.map(|port| GfxInput { port, fps: cfg.video.fps }),
            preview: cfg.preview.enabled.then_some(PreviewOut { width: cfg.preview.width, fps: cfg.preview.effective_fps(cfg.video.fps) }),
            loglevel: &ff.loglevel,
        };
        let args = encoder_args(&job);
        tracing::info!(channel = %cfg.id, "starting encoder: {} {}", ff.ffmpeg, lb_media::redact::urls(&args.join(" ")));
        match Encoder::spawn(&ff.ffmpeg, &args) {
            Ok(mut enc) => {
                *self.stdin.lock().await = enc.stdin.take();
                if let Some((_, consumed)) = gfx {
                    consumed.store(0, Ordering::Relaxed);
                    let mut stats = enc.stats.clone();
                    tokio::spawn(async move {
                        while stats.changed().await.is_ok() {
                            let frame = stats.borrow().frame;
                            consumed.store(frame, Ordering::Relaxed);
                        }
                    });
                }
                let mut prev = enc.preview.clone();
                let tx = self.preview.clone();
                tokio::spawn(async move {
                    while prev.changed().await.is_ok() {
                        let f = prev.borrow().clone();
                        let _ = tx.send(f);
                    }
                });
                self.encoder = Some(enc);
                self.retry_at = None;
                Ok(())
            }
            Err(e) => {
                self.on_failure(e.to_string());
                Err(e.to_string())
            }
        }
    }

    /// Recording settings of `data` in effect: only the recorder restarts,
    /// and only when they changed.
    pub async fn apply_record(&mut self, ff: &FfmpegConfig, data: &ChannelData) {
        let cfg = &data.config;
        let rec = &cfg.record;
        let dir = data.dir.join(&rec.dir);
        if rec.enabled {
            let _ = std::fs::create_dir_all(&dir);
        }
        self.recordings.set(rec.enabled.then(|| (dir.clone(), rec.file_prefix(&cfg.id), rec.clone())));
        let dir = dir.to_string_lossy();
        let job = RecordJob {
            rec,
            dir: &dir,
            channel: &cfg.id,
            port: self.recorder.port(),
            bitrate: rec.expected_bitrate(&cfg.encoder, &cfg.audio),
            loglevel: &ff.loglevel,
        };
        self.recorder.apply(&ff.ffmpeg, recorder_args(&job)).await;
    }

    /// Check whether the encoder died. Returns the error text if it did.
    pub async fn check(&mut self) -> Option<String> {
        self.recorder.check();
        let enc = self.encoder.as_mut()?;
        let exited = enc.try_exited()?;
        let err = enc.error_text().unwrap_or_else(|| if exited { "encoder exited".into() } else { "encoder failed".into() });
        self.encoder = None;
        *self.stdin.lock().await = None;
        self.on_failure(err.clone());
        Some(err)
    }

    fn on_failure(&mut self, err: String) {
        self.last_error = Some(err);
        self.restarts += 1;
        let delay = BACKOFF[self.failures_in_row.min(BACKOFF.len() - 1)];
        self.failures_in_row += 1;
        self.retry_at = Some(Instant::now() + Duration::from_secs(delay));
    }

    /// The encoder has been stable for a while: reset back-off, forget the
    /// error it recovered from.
    pub fn mark_healthy(&mut self) {
        self.failures_in_row = 0;
        self.last_error = None;
    }

    /// Network outputs failing right now: "output '<name>' unreachable: <why>".
    fn problems(&self) -> Vec<String> {
        let Some(enc) = &self.encoder else { return vec![] };
        let Ok(log) = enc.outputs.lock() else { return vec![] };
        self.network
            .iter()
            .filter_map(|(name, url)| {
                let (at, line) = log.iter().rev().find(|(_, l)| l.contains(url.as_str()))?;
                if at.elapsed() > PROBLEM_FRESH {
                    return None;
                }
                // "[fifo @ …] Error opening <url>: I/O error" → "I/O error"
                let why = line.split_once(url.as_str()).map(|(_, r)| r.trim_start_matches([':', ' ', '\'', ']'])).unwrap_or_default();
                let why = if why.is_empty() { "connection failed" } else { why };
                Some(format!("output '{name}' unreachable: {why}"))
            })
            .collect()
    }

    pub fn retry_due(&self) -> bool {
        self.encoder.is_none() && self.retry_at.is_none_or(|t| Instant::now() >= t)
    }

    pub async fn stop(&mut self) {
        *self.stdin.lock().await = None;
        if let Some(mut e) = self.encoder.take() {
            e.shutdown(Duration::from_secs(5)).await;
        }
        // after the encoder: the end of the feed closes the file
        self.recorder.stop().await;
        self.retry_at = None;
        self.failures_in_row = 0;
    }

    pub fn stats(&self) -> EncoderStats {
        self.encoder.as_ref().map(|e| e.stats.borrow().clone()).unwrap_or_default()
    }

    pub fn status(&self) -> OutputStatus {
        OutputStatus {
            running: self.running(),
            video_codec: self.codec.clone().unwrap_or_default(),
            restarts: self.restarts,
            stats: self.stats(),
            last_error: self.last_error.clone(),
            targets: self.targets.clone(),
            problems: self.problems(),
            recording: self.running().then(|| self.recordings.status()).flatten().map(|mut r| {
                r.error = self.recorder.error();
                r
            }),
        }
    }

    /// Refresh the recording status, delete recordings past their limits.
    pub fn housekeep(&mut self) -> Vec<std::path::PathBuf> {
        if self.running() { self.recordings.tick() } else { vec![] }
    }

    /// Forget the resolved codec (configuration changed).
    pub fn reset_codec(&mut self) {
        self.codec = None;
    }
}

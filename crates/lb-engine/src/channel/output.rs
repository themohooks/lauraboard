//! Encoder supervision: start, health check, restart with back-off.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use lb_core::config::FfmpegConfig;
use lb_core::project::{ChannelData, paths};
use lb_media::args::encoder::{EncodeJob, GfxInput, PreviewOut, encoder_args};
use lb_media::process::Encoder;
use lb_media::process::encoder::PreviewFrame;
use lb_media::progress::EncoderStats;

use super::playout::SharedStdin;
use super::status::OutputStatus;

const BACKOFF: &[u64] = &[1, 2, 5, 10, 20, 30];

pub struct Output {
    pub stdin: SharedStdin,
    encoder: Option<Encoder>,
    codec: Option<String>,
    restarts: u32,
    failures_in_row: usize,
    retry_at: Option<Instant>,
    last_error: Option<String>,
    targets: Vec<String>,
    /// Latest preview frame (kept across encoder restarts).
    pub preview: tokio::sync::watch::Sender<PreviewFrame>,
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
            preview,
        }
    }

    pub fn running(&self) -> bool {
        self.encoder.is_some()
    }

    /// Start the encoder. `gfx_port` enables the graphics overlay input.
    pub async fn start(&mut self, ff: &FfmpegConfig, data: &ChannelData, root: &Path, gfx_port: Option<u16>) -> Result<(), String> {
        let cfg = &data.config;
        if self.codec.is_none() {
            self.codec = Some(lb_media::hw::resolve_codec(&ff.ffmpeg, &cfg.encoder.video_codec).await);
        }
        let codec = self.codec.clone().unwrap_or_else(|| "libx264".into());
        let rec_dir = data.dir.join(&cfg.record.dir);
        if cfg.record.enabled {
            let _ = std::fs::create_dir_all(&rec_dir);
        }
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
        let job = EncodeJob {
            channel: &cfg.id,
            video: &cfg.video,
            audio: &cfg.audio,
            encoder: &cfg.encoder,
            video_codec: &codec,
            outputs: &outputs,
            record: cfg.record.enabled.then(|| (&cfg.record, rec_dir.to_string_lossy().into_owned())),
            // overlay at the video rate: anything slower makes motion stutter
            gfx: gfx_port.map(|port| GfxInput { port, fps: cfg.video.fps }),
            preview: cfg.preview.enabled.then_some(PreviewOut { width: cfg.preview.width, fps: cfg.preview.fps }),
            loglevel: &ff.loglevel,
        };
        let args = encoder_args(&job);
        tracing::info!(channel = %cfg.id, "starting encoder: {} {}", ff.ffmpeg, args.join(" "));
        match Encoder::spawn(&ff.ffmpeg, &args) {
            Ok(mut enc) => {
                *self.stdin.lock().await = enc.stdin.take();
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

    /// Check whether the encoder died. Returns the error text if it did.
    pub async fn check(&mut self) -> Option<String> {
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

    /// The encoder has been stable for a while: reset back-off.
    pub fn mark_healthy(&mut self) {
        self.failures_in_row = 0;
    }

    pub fn retry_due(&self) -> bool {
        self.encoder.is_none() && self.retry_at.is_none_or(|t| Instant::now() >= t)
    }

    pub async fn stop(&mut self) {
        *self.stdin.lock().await = None;
        if let Some(mut e) = self.encoder.take() {
            e.shutdown(Duration::from_secs(5)).await;
        }
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
        }
    }

    /// Forget the resolved codec (configuration changed).
    pub fn reset_codec(&mut self) {
        self.codec = None;
    }
}

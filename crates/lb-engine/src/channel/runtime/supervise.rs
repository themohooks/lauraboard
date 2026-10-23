//! Periodic supervision: encoder health and restarts, stuck item watchdog,
//! periodic persistence, history pruning.

use std::time::{Duration, Instant};

use lb_core::diff_ms;

use super::super::events::{EventKind, Level};
use super::{Runtime, now};

/// An item running this much longer than planned is considered stuck.
const WATCHDOG_GRACE_MS: i64 = 30_000;
const SAVE_EVERY: Duration = Duration::from_secs(10);
const HEALTHY_AFTER: Duration = Duration::from_secs(60);

impl Runtime {
    pub(super) async fn on_tick(&mut self) {
        if self.running {
            if self.ensure_graphics() {
                // the overlay thread was respawned on a new port
                self.restart_encoder().await;
            }
            self.check_encoder().await;
            for p in self.output.housekeep() {
                tracing::info!(channel = %self.shared.id, "old recording deleted: {}", p.display());
            }
            self.watchdog().await;
            if self.last_save.elapsed() >= SAVE_EVERY {
                self.save_state();
            }
        }
        self.update_graphics_state();
        self.publish_status();
    }

    async fn check_encoder(&mut self) {
        if let Some(err) = self.output.check().await {
            self.encoder_up_since = None;
            self.last_error = Some(err.clone());
            self.emit(Level::Critical, EventKind::EncoderStopped { error: Some(err) });
            // the play task notices the closed pipe and reports EncoderGone
        }
        if self.output.running() {
            if self.encoder_up_since.is_some_and(|t| t.elapsed() >= HEALTHY_AFTER) {
                self.output.mark_healthy();
                self.last_error = None;
            }
            return;
        }
        if self.output.retry_due() {
            self.start_encoder().await;
            if self.output.running() && self.on_air.is_none() {
                self.play_resume_or_next().await;
            }
        }
    }

    pub(super) async fn start_encoder(&mut self) {
        let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).clone();
        let gfx = self.gfx_thread.as_ref().map(|g| (g.port, g.consumed.clone()));
        self.encoder_config = Some(data.config.clone());
        match self.output.start(&self.shared.app.ffmpeg, &data, &self.shared.root, gfx).await {
            Ok(()) => {
                self.encoder_up_since = Some(Instant::now());
                let codec = self.output.status().video_codec;
                self.emit(Level::Info, EventKind::EncoderStarted { codec });
            }
            Err(e) => {
                self.last_error = Some(e.clone());
                self.emit(Level::Critical, EventKind::EncoderStopped { error: Some(e) });
            }
        }
    }

    /// Restart the encoder (settings changed); the current item resumes.
    pub(super) async fn restart_encoder(&mut self) {
        if let Some(cur) = self.on_air.take() {
            if let Some(p) = &cur.play {
                p.stop();
            }
            self.resume = Some((cur.item.clone(), cur.offset_ms + diff_ms(now(), cur.started)));
        }
        self.output.stop().await;
        self.start_encoder().await;
        if self.output.running() {
            self.play_resume_or_next().await;
        }
    }

    /// Skip items that run far beyond their planned duration (stalled
    /// stream, hung decoder).
    async fn watchdog(&mut self) {
        let Some(cur) = &self.on_air else { return };
        let overdue = diff_ms(now(), cur.started) - (cur.item.duration_ms - cur.offset_ms);
        if overdue > WATCHDOG_GRACE_MS {
            let title = cur.item.title.clone();
            self.emit(Level::Warning, EventKind::Message { text: format!("'{title}' overran by {} s, skipping", overdue / 1000) });
            self.interrupt_current("watchdog: overran planned duration");
            self.start_next().await;
        }
    }
}

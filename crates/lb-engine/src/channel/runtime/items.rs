//! Transitions: starting the next item and finishing the current one.

use lb_core::history::{AsRunRecord, AsRunStatus};
use lb_core::rundown::{Origin, RundownItem};
use lb_core::{Ms, add_ms, diff_ms};

use super::super::events::{EventKind, Level};
use super::super::playout::{self, PlayOutcome};
use super::super::{job, vars};
use super::{OnAir, Runtime, now};
use crate::persist::asrun;

/// Consecutive failures before falling back to the standby slate.
const MAX_FAIL_STREAK: u32 = 5;
/// Length of standby chunks when nothing is available.
const STANDBY_MS: Ms = 10_000;

impl Runtime {
    /// Start the next item from the queue / rundown (or standby).
    pub(super) async fn start_next(&mut self) {
        for _ in 0..50 {
            let t = now();
            let next = if self.fail_streak >= MAX_FAIL_STREAK {
                None
            } else {
                let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
                self.sched.take_next(&data, t)
            };
            let item = match next {
                Some(i) => i,
                None => {
                    self.fail_streak = 0;
                    let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
                    self.sched.standby_item(&data, t, STANDBY_MS)
                }
            };
            if self.start_item(item, 0).await {
                return;
            }
        }
        tracing::error!(channel = %self.shared.id, "could not start any item");
    }

    /// Start `item` at `offset_ms`. Returns false when it could not start
    /// (missing file etc.) — an as-run "skipped" record is written then.
    pub(super) async fn start_item(&mut self, item: RundownItem, offset_ms: i64) -> bool {
        self.ensure_probed(&item.media).await;
        let args = {
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            job::build(&data, &self.shared.root, &item, offset_ms, &self.shared.app.ffmpeg.loglevel)
        };
        let args = match args {
            Ok(a) => a,
            Err(e) => {
                self.fail_streak += 1;
                self.emit(Level::Warning, EventKind::Message { text: format!("skipping '{}': {e}", item.title) });
                let t = now();
                self.write_asrun(&item, t, t, AsRunStatus::Skipped, &e.to_string());
                return false;
            }
        };
        self.generation += 1;
        let play = playout::spawn(
            self.shared.app.ffmpeg.ffmpeg.clone(),
            args,
            format!("{}:{}", self.shared.id, item.media),
            self.output.stdin.clone(),
            self.generation,
            self.done_tx.clone(),
        );
        let started = now();
        if offset_ms == 0 {
            self.sched.on_start(&item);
        }
        self.replan_after_start(&item, started, offset_ms);
        tracing::info!(channel = %self.shared.id, media = %item.media, title = %item.title, "on air");
        self.on_air = Some(OnAir { item: item.clone(), started, offset_ms, generation: self.generation, play: Some(play) });
        self.update_graphics_state();
        self.emit(Level::Info, EventKind::ItemStarted { item });
        self.save_state();
        self.publish_status();
        true
    }

    /// Keep the rundown aligned with the actual start.
    fn replan_after_start(&mut self, item: &RundownItem, started: lb_core::WallTime, offset_ms: i64) {
        if self.sched.manual {
            return;
        }
        let expected_end = add_ms(started, item.duration_ms - offset_ms);
        let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
        match item.origin {
            Origin::Manual | Origin::Slate if item.block_key.is_none() => self.sched.replan_hard(&data, expected_end),
            _ => self.sched.replan_soft(&data, item, expected_end),
        }
        let warnings = self.sched.rundown.warnings.len();
        let items = self.sched.rundown.items.len();
        drop(data);
        self.emit(Level::Info, EventKind::RundownChanged { items, warnings });
    }

    /// Probe media that was never probed (so audio presence is known).
    async fn ensure_probed(&mut self, media: &str) {
        if media.is_empty() {
            return;
        }
        let path = {
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            match data.library.get(media) {
                Some(m) if job::needs_probe(m) => lb_core::project::paths::resolve_str(&self.shared.root, &m.path),
                _ => return,
            }
        };
        let res = lb_media::probe::probe(&self.shared.app.ffmpeg.ffprobe, &path).await;
        let mut data = self.shared.data.write().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = data.library.get_mut(media) {
            match res {
                Ok(p) => {
                    if m.duration.ms() <= 0 {
                        m.duration = lb_core::Dur(p.duration_ms);
                    }
                    m.probe = p.info;
                }
                Err(e) => m.probe.error = Some(e.to_string()),
            }
        }
        let _ = data.save_library();
    }

    /// A play task reported its outcome.
    pub(super) async fn on_finished(&mut self, generation: u64, outcome: PlayOutcome) {
        let Some(cur) = self.on_air.as_ref() else { return };
        if cur.generation != generation {
            return; // stale (item was skipped)
        }
        let cur = self.on_air.take().unwrap();
        let t = now();
        let played = diff_ms(t, cur.started);
        match outcome {
            PlayOutcome::Completed => {
                self.fail_streak = 0;
                let status = if cur.item.truncated { AsRunStatus::Truncated } else { AsRunStatus::Completed };
                self.write_asrun(&cur.item, cur.started, t, status, "");
                self.start_next_if_running().await;
            }
            PlayOutcome::Failed { error, bytes } => {
                self.fail_streak += 1;
                let status = if bytes == 0 { AsRunStatus::Skipped } else { AsRunStatus::Interrupted };
                self.emit(Level::Warning, EventKind::Message { text: format!("'{}' failed: {error}", cur.item.title) });
                self.write_asrun(&cur.item, cur.started, t, status, &error);
                self.start_next_if_running().await;
            }
            PlayOutcome::Stopped => {
                // stop requested while still current (should not happen): move on
                self.write_asrun(&cur.item, cur.started, t, AsRunStatus::Interrupted, "stopped");
                self.start_next_if_running().await;
            }
            PlayOutcome::EncoderGone => {
                // resume this item once the encoder is back
                self.write_asrun(&cur.item, cur.started, t, AsRunStatus::Interrupted, "encoder stopped");
                let offset = (cur.offset_ms + played).min(cur.item.duration_ms - 500).max(0);
                self.resume = Some((cur.item, offset));
            }
        }
        self.publish_status();
    }

    async fn start_next_if_running(&mut self) {
        if self.running && self.output.running() {
            self.start_next().await;
        }
    }

    /// Stop the current item (skip / interrupt) and record it.
    pub(super) fn interrupt_current(&mut self, note: &str) -> Option<OnAir> {
        let cur = self.on_air.take()?;
        if let Some(p) = &cur.play {
            p.stop();
        }
        let t = now();
        self.write_asrun(&cur.item, cur.started, t, AsRunStatus::Interrupted, note);
        Some(cur)
    }

    pub(super) fn write_asrun(
        &mut self,
        item: &RundownItem,
        started: lb_core::WallTime,
        ended: lb_core::WallTime,
        status: AsRunStatus,
        note: &str,
    ) {
        self.program.aired(item, diff_ms(ended, started));
        let rec = AsRunRecord {
            channel: self.shared.id.clone(),
            uid: item.uid.clone(),
            media: item.media.clone(),
            title: item.title.clone(),
            kind: item.kind,
            category: item.category.clone(),
            block: item.block.clone(),
            rule: item.rule.clone(),
            planned_start: item.start,
            started,
            ended,
            planned_duration_ms: item.duration_ms,
            played_ms: diff_ms(ended, started).max(0),
            status,
            note: note.to_string(),
        };
        let dir = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).asrun_dir();
        if let Err(e) = asrun::append(&dir, &rec) {
            tracing::warn!("as-run write failed: {e}");
        }
        self.sched.on_end(&rec);
        self.emit(Level::Info, EventKind::ItemEnded { record: rec });
    }

    /// Push the on-air context to the graphics thread.
    pub(super) fn update_graphics_state(&mut self) {
        let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
        let on_air = self.on_air.as_ref().map(|o| &o.item);
        let next = self.sched.peek_next();
        let next_program = self.sched.next_program(on_air);
        let v = vars::build(&data, on_air, next, next_program, &self.user_vars);
        let np_start = next_program.map(|p| p.start);
        let started = self.on_air.as_ref().map(|o| (o.item.clone(), add_ms(o.started, -o.offset_ms)));
        let program = self.program_clock();
        if let Ok(mut g) = self.gfx.lock() {
            g.program = program;
            g.on_air = started;
            g.next_program_start = np_start;
            g.vars = v;
            g.profile = data.config.graphics.profile.clone();
        }
    }

    /// Copy the graphics configuration to the graphics thread.
    pub(super) fn sync_graphics_config(&mut self) {
        let cfg = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).graphics.clone();
        if let Ok(mut g) = self.gfx.lock() {
            g.cfg = cfg;
            g.cfg_version += 1;
        }
    }
}

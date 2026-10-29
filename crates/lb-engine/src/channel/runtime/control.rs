//! Operator commands: start/stop, skip, manual mode, inserts, graphics.

use lb_core::config::ChannelConfig;
use lb_core::project::ChannelData;
use lb_core::validate::{Severity, validate_channel_full};

use super::super::commands::{Command, Reply, Request};
use super::super::events::{EventKind, Level};
use super::{Runtime, now};
use crate::persist::lock::InstanceLock;

impl Runtime {
    pub(super) async fn handle(&mut self, req: Request) {
        let reply = self.execute(req.cmd).await;
        if let Some(tx) = req.reply {
            let _ = tx.send(reply);
        }
        self.publish_status();
    }

    async fn execute(&mut self, cmd: Command) -> Reply {
        match cmd {
            Command::Start => self.start(false).await.map(|_| "started".into()),
            Command::RestartEncoder => {
                if self.running {
                    self.output.reset_codec();
                    self.restart_encoder().await;
                } else {
                    self.start(false).await?;
                }
                if !self.output.running() {
                    return Err(self.last_error.clone().unwrap_or_else(|| "encoder did not start".into()));
                }
                self.last_error = None;
                Ok("encoder restarted".into())
            }
            Command::Stop => {
                self.request_stop().await;
                Ok(if self.closing.is_some() { "closing broadcast" } else { "stopped" }.into())
            }
            Command::Skip => {
                self.require_running()?;
                self.interrupt_current("skipped by operator");
                self.start_next().await;
                Ok("skipped".into())
            }
            Command::Restart => {
                self.require_running()?;
                let cur = self.interrupt_current("restarted by operator").ok_or("nothing on air")?;
                let mut item = cur.item;
                item.start = now();
                self.start_item(item, 0).await;
                Ok("restarted".into())
            }
            Command::Seek { to_ms, by_ms } => {
                self.require_running()?;
                let cur = self.on_air.as_ref().ok_or("nothing on air")?;
                if cur.item.media.is_empty()
                    || matches!(cur.item.kind, lb_core::media::MediaKind::Stream | lb_core::media::MediaKind::Image)
                {
                    return Err("this item cannot be rewound".into());
                }
                let pos = cur.offset_ms + lb_core::diff_ms(now(), cur.started);
                let target = to_ms.or(by_ms.map(|d| pos + d)).ok_or("seek: to_ms or by_ms required")?;
                // not the very end (the item would finish at once), and never
                // offset 0: that counts as a new start of the item
                let target = target.clamp(1, (cur.item.duration_ms - 1000).max(1));
                // stopped without an as-run record: the item keeps airing
                let cur = self.on_air.take().ok_or("nothing on air")?;
                if let Some(p) = &cur.play {
                    p.stop();
                }
                if !self.start_item(cur.item, target).await {
                    self.start_next().await;
                    return Err("could not continue the item at the new position".into());
                }
                Ok(format!("position {}", lb_core::Dur(target / 1000 * 1000)))
            }
            Command::Manual { enabled } => {
                self.sched.manual = enabled;
                if !enabled {
                    let from = self.expected_end();
                    let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
                    self.sched.replan_hard(&data, from);
                }
                self.emit(Level::Info, EventKind::ModeChanged { manual: enabled });
                self.save_state();
                Ok(if enabled { "manual mode" } else { "automatic mode" }.into())
            }
            Command::PlayNow { media } => {
                let item = {
                    let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
                    self.sched.manual_item(&data, &media, now())
                }
                .ok_or_else(|| format!("unknown media '{media}'"))?;
                self.require_running()?;
                self.interrupt_current("interrupted by manual play");
                self.start_item(item, 0).await;
                Ok("playing".into())
            }
            Command::Queue { media } => {
                let item = {
                    let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
                    self.sched.manual_item(&data, &media, now())
                }
                .ok_or_else(|| format!("unknown media '{media}'"))?;
                self.sched.queue.push_back(item);
                Ok(format!("queued ({} in queue)", self.sched.queue.len()))
            }
            Command::ClearQueue => {
                self.sched.queue.clear();
                Ok("queue cleared".into())
            }
            Command::AdBreak { ad_break, now: immediate } => {
                let items = {
                    let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
                    self.sched.ad_break_items(&data, &ad_break, now())
                };
                if items.is_empty() {
                    return Err(format!("ad break '{ad_break}' unknown or has no eligible spots"));
                }
                let n = items.len();
                for it in items.into_iter().rev() {
                    self.sched.queue.push_front(it);
                }
                if immediate && self.running {
                    self.interrupt_current("interrupted by ad break");
                    self.start_next().await;
                }
                Ok(format!("ad break with {n} items inserted"))
            }
            Command::Graphic { element, action } => {
                self.require_element(&element)?;
                if let Ok(mut g) = self.gfx.lock() {
                    g.manual.insert(element.clone(), action);
                }
                self.emit(Level::Info, EventKind::GraphicsChanged);
                Ok(format!("{element}: {action:?}"))
            }
            Command::GraphicAuto { element } => {
                if let Ok(mut g) = self.gfx.lock() {
                    g.manual.remove(&element);
                }
                self.emit(Level::Info, EventKind::GraphicsChanged);
                Ok(format!("{element}: automatic"))
            }
            Command::SetVar { key, value } => {
                match value {
                    Some(v) => self.user_vars.insert(key.clone(), v),
                    None => self.user_vars.remove(&key),
                };
                self.update_graphics_state();
                Ok(format!("variable '{key}' updated"))
            }
            Command::SwitchSchedule { schedule } => {
                if let Some(s) = &schedule {
                    let exists = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).schedules.get(s).is_some();
                    if !exists {
                        return Err(format!("unknown schedule '{s}'"));
                    }
                }
                self.sched.force_schedule = schedule.clone();
                let from = self.expected_end();
                {
                    let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
                    self.sched.replan_hard(&data, from);
                }
                self.emit(Level::Info, EventKind::ScheduleSwitched { schedule });
                self.save_state();
                Ok("schedule switched".into())
            }
            Command::Reload => self.reload().await,
            Command::Shutdown => {
                self.stop().await;
                Ok("shut down".into())
            }
        }
    }

    fn require_running(&self) -> Result<(), String> {
        if self.running { Ok(()) } else { Err("channel is not broadcasting".into()) }
    }

    fn require_element(&self, id: &str) -> Result<(), String> {
        let ok = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).graphics.element(id).is_some();
        if ok { Ok(()) } else { Err(format!("unknown graphic element '{id}'")) }
    }

    /// When the on-air item is expected to end (or now).
    pub(super) fn expected_end(&self) -> lb_core::WallTime {
        self.on_air
            .as_ref()
            .map(|o| lb_core::add_ms(o.started, o.item.duration_ms - o.offset_ms))
            .filter(|t| *t > now())
            .unwrap_or_else(now)
    }

    /// Re-read channel files from disk and replan.
    async fn reload(&mut self) -> Reply {
        let dir = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).dir.clone();
        let fresh = ChannelData::load(&dir).map_err(|e| e.to_string())?;
        let encoder_changed = {
            let old = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            encoder_settings_changed(&old.config, &fresh.config)
                || self.encoder_config.as_ref().is_some_and(|run| encoder_settings_changed(run, &fresh.config))
        };
        *self.shared.data.write().unwrap_or_else(|e| e.into_inner()) = fresh;
        self.sync_graphics_config();
        let from = self.expected_end();
        {
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            if !self.sched.manual {
                self.sched.replan_hard(&data, from);
            }
        }
        self.update_graphics_state();
        // graphics edits apply on the fly (the overlay stays attached);
        // only graphics switched on for a running channel without an
        // overlay, or new frame settings, need the encoder restarted
        if encoder_changed && let Some(t) = self.gfx_thread.take() {
            t.shutdown(); // frame size or rate may have changed
        }
        let overlay_changed = self.running && self.ensure_graphics();
        if (encoder_changed || overlay_changed) && self.running {
            self.output.reset_codec();
            self.restart_encoder().await;
            return Ok("reloaded (encoder restarted)".into());
        }
        if self.running {
            // recording settings: the recorder restarts, the broadcast goes on
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).clone();
            self.output.apply_record(&self.shared.app.ffmpeg, &data).await;
        }
        Ok("reloaded".into())
    }

    /// Start broadcasting.
    pub(super) async fn start(&mut self, recovering: bool) -> Result<(), String> {
        if self.running {
            return Ok(());
        }
        self.closing = None;
        let (report, dir) = {
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            (validate_channel_full(&data, &self.shared.root), data.state_dir())
        };
        for i in &report.issues {
            match i.severity {
                Severity::Error => tracing::error!(channel = %self.shared.id, "{}: {}", i.location, i.message),
                Severity::Warning => tracing::warn!(channel = %self.shared.id, "{}: {}", i.location, i.message),
                Severity::Info => {}
            }
        }
        if report.has_errors() {
            let first = report.issues.iter().find(|i| i.severity == Severity::Error).map(|i| format!("{}: {}", i.location, i.message));
            return Err(format!("validation failed with {} error(s); first: {}", report.count(Severity::Error), first.unwrap_or_default()));
        }
        let lock = InstanceLock::acquire(&dir, &self.shared.id).map_err(|e| e.to_string())?;
        self.lock = Some(lock);
        self.output.reset_session_stats();
        self.ensure_graphics();
        self.running = true;
        self.emit(Level::Info, EventKind::Started);
        self.start_encoder().await;
        // resume an interrupted item or join the schedule
        let saved = crate::persist::state::load(&dir);
        let t = now();
        let resume = saved.on_air.zip(saved.started).and_then(|(item, started)| {
            let offset = saved.started_offset_ms + lb_core::diff_ms(t, started);
            (offset >= 0 && offset < item.duration_ms - 2_000).then_some((item, offset))
        });
        {
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            match &resume {
                Some((item, offset)) => {
                    let end = lb_core::add_ms(t, item.duration_ms - offset);
                    self.sched.replan_hard(&data, end);
                }
                None => {
                    self.sched.block_key = None;
                    self.sched.block_items_done = 0;
                    self.sched.replan_hard(&data, t);
                }
            }
        }
        if !recovering || resume.is_none() {
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            self.opening = self.sched.broadcast_bookend(&data, t, true).into_iter().map(|item| (item, 0)).collect();
        }
        if let Some((item, offset)) = resume {
            tracing::info!(channel = %self.shared.id, media = %item.media, offset, "resuming after restart");
            if self.opening.is_empty() {
                self.resume = Some((item, offset));
            } else {
                self.opening.push_back((item, offset));
            }
        }
        if self.output.running() {
            self.play_resume_or_next().await;
        }
        Ok(())
    }

    pub(super) async fn play_resume_or_next(&mut self) {
        self.start_next().await;
    }

    /// Stop broadcasting gracefully and persist state.
    async fn request_stop(&mut self) {
        if !self.running || self.closing.is_some() {
            return;
        }
        let items = {
            let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            self.sched.broadcast_bookend(&data, now(), false)
        };
        if items.is_empty() {
            self.stop().await;
            return;
        }
        self.save_state_with(false);
        self.interrupt_current("broadcast closing");
        self.resume = None;
        self.closing = Some(items.into());
        self.start_next().await;
    }

    pub(super) async fn stop(&mut self) {
        if !self.running {
            return;
        }
        // the on-air item and its start are saved, so a later start resumes it
        self.save_state_with(false);
        self.interrupt_current("broadcast stopped");
        self.output.stop().await;
        self.encoder_up_since = None;
        if let Some(t) = self.gfx_thread.take() {
            t.shutdown();
        }
        self.running = false;
        self.closing = None;
        self.opening.clear();
        self.resume = None;
        self.lock = None;
        self.emit(Level::Info, EventKind::Stopped);
    }
}

fn encoder_settings_changed(a: &ChannelConfig, b: &ChannelConfig) -> bool {
    a.local_hls.enabled != b.local_hls.enabled
        || a.video != b.video
        || a.audio != b.audio
        || a.encoder != b.encoder
        || a.outputs != b.outputs
        || a.preview != b.preview
}

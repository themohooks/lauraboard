//! Operator commands: start/stop, skip, manual mode, inserts, graphics.

use lb_core::config::ChannelConfig;
use lb_core::project::ChannelData;
use lb_core::validate::{Severity, validate_channel_full};

use super::super::commands::{Command, Reply, Request};
use super::super::events::{EventKind, Level};
use super::super::gfx::GfxThread;
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
            Command::Start => self.start().await.map(|_| "started".into()),
            Command::Stop => {
                self.stop().await;
                Ok("stopped".into())
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
        if encoder_changed && self.running {
            self.output.reset_codec();
            self.restart_encoder().await;
            return Ok("reloaded (encoder restarted)".into());
        }
        Ok("reloaded".into())
    }

    /// Start broadcasting.
    pub(super) async fn start(&mut self) -> Result<(), String> {
        if self.running {
            return Ok(());
        }
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
        self.spawn_graphics();
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
        if let Some((item, offset)) = resume {
            tracing::info!(channel = %self.shared.id, media = %item.media, offset, "resuming after restart");
            self.resume = Some((item, offset));
        }
        if self.output.running() {
            self.play_resume_or_next().await;
        }
        Ok(())
    }

    pub(super) async fn play_resume_or_next(&mut self) {
        match self.resume.take() {
            Some((item, offset)) => {
                if !self.start_item(item, offset).await {
                    self.start_next().await;
                }
            }
            None => self.start_next().await,
        }
    }

    fn spawn_graphics(&mut self) {
        let (enabled, w, h) = {
            let d = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            (d.config.graphics.enabled && !d.graphics.elements.is_empty(), d.config.video.width, d.config.video.height)
        };
        if !enabled || self.gfx_thread.is_some() {
            return;
        }
        match GfxThread::spawn(self.gfx.clone(), w, h, self.shared.root.clone(), self.shared.id.clone()) {
            Ok(t) => self.gfx_thread = Some(t),
            Err(e) => tracing::warn!("graphics disabled: {e}"),
        }
    }

    /// Stop broadcasting gracefully and persist state.
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
        self.resume = None;
        self.lock = None;
        self.emit(Level::Info, EventKind::Stopped);
    }
}

fn encoder_settings_changed(a: &ChannelConfig, b: &ChannelConfig) -> bool {
    a.video != b.video
        || a.audio != b.audio
        || a.encoder != b.encoder
        || a.outputs != b.outputs
        || a.record != b.record
        || a.preview != b.preview
}

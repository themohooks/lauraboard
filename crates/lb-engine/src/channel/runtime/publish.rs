//! Status snapshots, events, notifications and the recovery state file.

use std::time::Instant;

use lb_core::{add_ms, diff_ms};

use super::super::events::{Event, EventKind, Level};
use super::super::status::{ChannelStatus, OnAirInfo, RunState};
use super::{Runtime, now};
use crate::notify;
use crate::persist::state::{self, RuntimeState};

impl Runtime {
    pub(super) fn emit(&self, level: Level, kind: EventKind) {
        let ev = Event { channel: self.shared.id.clone(), at: now(), level, kind };
        match level {
            Level::Critical => tracing::error!(channel = %ev.channel, "{:?}", ev.kind),
            Level::Warning => tracing::warn!(channel = %ev.channel, "{:?}", ev.kind),
            Level::Info => tracing::debug!(channel = %ev.channel, "{:?}", ev.kind),
        }
        if level == Level::Critical {
            notify::send(&self.shared.app.notify, &ev);
        }
        let _ = self.events.send(ev);
    }

    pub(super) fn publish_status(&mut self) {
        self.publish_rundown();
        let t = now();
        let data = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
        let on_air = self.on_air.as_ref().map(|o| {
            let elapsed = o.offset_ms + diff_ms(t, o.started);
            OnAirInfo {
                item: o.item.clone(),
                started: add_ms(o.started, -o.offset_ms),
                elapsed_ms: elapsed,
                remaining_ms: (o.item.duration_ms - elapsed).max(0),
                deviation_ms: if o.offset_ms == 0 { diff_ms(o.started, o.item.start) } else { 0 },
            }
        });
        let next_program = self.sched.next_program(self.on_air.as_ref().map(|o| &o.item)).cloned();
        let state = match (self.running, self.output.running(), &self.on_air) {
            (false, _, _) => RunState::Stopped,
            (true, false, _) => RunState::Reconnecting,
            (true, true, Some(_)) => RunState::OnAir,
            (true, true, None) => RunState::Starting,
        };
        let status = ChannelStatus {
            channel: self.shared.id.clone(),
            name: data.config.name.clone(),
            autostart: data.config.autostart,
            state,
            manual: self.sched.manual,
            force_schedule: self.sched.force_schedule.clone(),
            now: Some(t),
            on_air,
            next: self.sched.peek_next().cloned(),
            time_to_next_program_ms: next_program.as_ref().map(|p| diff_ms(p.start, t)),
            program: self.program_clock(),
            next_program,
            manual_queue: self.sched.queue.len(),
            output: self.output.status(),
            graphics: self.gfx.lock().map(|g| g.active.clone()).unwrap_or_default(),
            plan_warnings: self.sched.rundown.warnings.len(),
            last_error: self.last_error.clone(),
        };
        let _ = self.status_tx.send(status);
    }

    pub(super) fn save_state(&mut self) {
        let running = self.running;
        self.save_state_with(running);
    }

    pub(super) fn save_state_with(&mut self, running: bool) {
        self.last_save = Instant::now();
        let s = RuntimeState {
            on_air: self.on_air.as_ref().map(|o| o.item.clone()),
            started: self.on_air.as_ref().map(|o| o.started),
            started_offset_ms: self.on_air.as_ref().map(|o| o.offset_ms).unwrap_or(0),
            plan: self.sched.plan.clone(),
            block_items_done: self.sched.block_items_done,
            running,
            force_schedule: self.sched.force_schedule.clone(),
            manual: self.sched.manual,
            saved_at: Some(now()),
        };
        let dir = self.shared.data.read().unwrap_or_else(|e| e.into_inner()).state_dir();
        if let Err(e) = state::save(&dir, &s) {
            tracing::warn!("saving runtime state failed: {e}");
        }
    }

    /// Publish the rundown when it changed.
    pub(super) fn publish_rundown(&mut self) {
        if self.sched.version != self.rundown_version {
            self.rundown_version = self.sched.version;
            let _ = self.rundown_tx.send(std::sync::Arc::new(self.sched.rundown.clone()));
        }
    }
}

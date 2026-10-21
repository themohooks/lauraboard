//! The channel actor.
//!
//! * [`items`] — starting / finishing items (transitions)
//! * [`control`] — operator commands
//! * [`supervise`] — periodic tick: encoder health, watchdog, persistence
//! * [`publish`] — status snapshots, events, notifications, state file

mod control;
mod items;
mod publish;
mod supervise;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc, watch};

use lb_core::WallTime;
use lb_core::rundown::{Rundown, RundownItem};
use lb_media::process::encoder::PreviewFrame;

use super::gfx::{GfxState, GfxThread, SharedGfx};
use super::output::Output;
use super::playout::PlayOutcome;
use super::scheduler::Scheduler;
use super::{ChannelShared, ChannelStatus, Event, Request};
use crate::persist::lock::InstanceLock;
use crate::persist::{asrun, state};

pub(crate) fn now() -> WallTime {
    chrono::Local::now().naive_local()
}

/// Item currently on air.
#[derive(Debug, Clone)]
pub(crate) struct OnAir {
    pub item: RundownItem,
    pub started: WallTime,
    /// Offset into the item at which playback started (resume).
    pub offset_ms: i64,
    pub generation: u64,
    pub play: Option<super::playout::PlayHandle>,
}

pub struct Runtime {
    shared: Arc<ChannelShared>,
    rx: mpsc::Receiver<Request>,
    status_tx: watch::Sender<ChannelStatus>,
    rundown_tx: watch::Sender<Arc<Rundown>>,
    rundown_version: u64,
    events: broadcast::Sender<Event>,
    done_tx: mpsc::Sender<(u64, PlayOutcome)>,
    done_rx: mpsc::Receiver<(u64, PlayOutcome)>,
    sched: Scheduler,
    output: Output,
    gfx: SharedGfx,
    gfx_thread: Option<GfxThread>,
    running: bool,
    on_air: Option<OnAir>,
    /// Item to resume once the encoder is back.
    resume: Option<(RundownItem, i64)>,
    generation: u64,
    user_vars: HashMap<String, String>,
    lock: Option<InstanceLock>,
    fail_streak: u32,
    last_save: Instant,
    encoder_up_since: Option<Instant>,
    last_error: Option<String>,
}

impl Runtime {
    pub(crate) fn new(
        shared: Arc<ChannelShared>,
        rx: mpsc::Receiver<Request>,
        status_tx: watch::Sender<ChannelStatus>,
        rundown_tx: watch::Sender<Arc<Rundown>>,
        events: broadcast::Sender<Event>,
        preview_tx: watch::Sender<PreviewFrame>,
    ) -> Self {
        let (done_tx, done_rx) = mpsc::channel(16);
        let (dir, history_days) = {
            let d = shared.data.read().unwrap_or_else(|e| e.into_inner());
            (d.dir.clone(), d.config.planning.history.ms() / 86_400_000)
        };
        let saved = state::load(&dir.join("state"));
        let history = asrun::load_history(&dir.join("asrun"), now().date(), history_days.max(1));
        let mut sched = Scheduler::new(saved.plan.clone(), history);
        sched.manual = saved.manual;
        sched.force_schedule = saved.force_schedule.clone();
        sched.block_key = saved.on_air.as_ref().and_then(|i| i.block_key.clone());
        sched.block_items_done = saved.block_items_done;
        let gfx = Arc::new(Mutex::new(GfxState::default()));
        Runtime {
            shared,
            rx,
            status_tx,
            rundown_tx,
            rundown_version: 0,
            events,
            done_tx,
            done_rx,
            sched,
            output: Output::new(preview_tx),
            gfx,
            gfx_thread: None,
            running: false,
            on_air: None,
            resume: None,
            generation: 0,
            user_vars: HashMap::new(),
            lock: None,
            fail_streak: 0,
            last_save: Instant::now(),
            encoder_up_since: None,
            last_error: None,
        }
    }

    /// Actor main loop.
    pub async fn run(mut self) {
        self.sync_graphics_config();
        self.boot().await;
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                req = self.rx.recv() => match req {
                    Some(r) => self.handle(r).await,
                    None => break,
                },
                Some((generation, outcome)) = self.done_rx.recv() => self.on_finished(generation, outcome).await,
                _ = tick.tick() => self.on_tick().await,
            }
        }
        if self.running {
            self.stop().await;
        }
    }

    /// Auto-start after process start: resume if the channel was running
    /// before, or start if `autostart` is set.
    async fn boot(&mut self) {
        let (autostart, enabled, dir) = {
            let d = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            (d.config.autostart, d.config.enabled, d.dir.clone())
        };
        let saved = state::load(&dir.join("state"));
        if enabled
            && (autostart || saved.running)
            && let Err(e) = self.start().await
        {
            tracing::error!(channel = %self.shared.id, "autostart failed: {e}");
            self.last_error = Some(e);
        }
        self.publish_status();
    }
}

//! One broadcast channel: an actor task owning playout, plus a cloneable
//! handle used by the API, CLI and GUI.
//!
//! * [`commands`], [`status`], [`events`] — public message types
//! * [`scheduler`] — rundown alignment and next-item logic (pure)
//! * [`runtime`] — the actor: transitions, supervision, publishing
//! * [`playout`] — feeding one item through a decoder into the encoder
//! * [`output`] — encoder supervisor
//! * [`recordings`] — recording status and clean-up of old files
//! * [`gfx`] — graphics rendering thread
//! * [`job`] — rundown item → decoder command line
//! * [`vars`] — graphics template variables

pub mod avatar;
pub mod commands;
pub mod events;
pub mod gfx;
pub mod job;
pub mod output;
pub mod playout;
pub mod recordings;
pub mod runtime;
pub mod scheduler;
pub mod status;
pub mod vars;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::{broadcast, mpsc, oneshot, watch};

use lb_core::config::AppConfig;
use lb_core::project::ChannelData;
use lb_core::rundown::Rundown;
use lb_media::process::encoder::PreviewFrame;

use crate::error::{EngineError, Result};
pub use commands::{Command, Reply, Request};
pub use events::{Event, EventKind, Level};
pub use status::ChannelStatus;

/// Data shared between the actor and API handlers.
pub struct ChannelShared {
    pub id: String,
    pub root: PathBuf,
    pub app: AppConfig,
    /// Channel configuration and content. API edits go through here and
    /// are saved to disk, then a [`Command::Reload`] is sent.
    pub data: RwLock<ChannelData>,
}

/// Cloneable handle to a running channel actor.
#[derive(Clone)]
pub struct ChannelHandle {
    pub shared: Arc<ChannelShared>,
    tx: mpsc::Sender<Request>,
    status: watch::Receiver<ChannelStatus>,
    rundown: watch::Receiver<Arc<Rundown>>,
    events: broadcast::Sender<Event>,
    preview: watch::Receiver<PreviewFrame>,
}

impl ChannelHandle {
    /// Spawn the channel actor on the current tokio runtime.
    pub fn spawn(app: AppConfig, root: PathBuf, data: ChannelData) -> Self {
        let id = data.id().to_string();
        let (name, autostart) = (data.config.name.clone(), data.config.autostart);
        let shared = Arc::new(ChannelShared { id: id.clone(), root, app, data: RwLock::new(data) });
        let (tx, rx) = mpsc::channel(64);
        let (status_tx, status) = watch::channel(ChannelStatus { channel: id, name, autostart, ..Default::default() });
        let (rundown_tx, rundown) = watch::channel(Arc::new(Rundown::default()));
        let (events, _) = broadcast::channel(512);
        let (preview_tx, preview) = watch::channel(None);
        let rt = runtime::Runtime::new(shared.clone(), rx, status_tx, rundown_tx, events.clone(), preview_tx);
        tokio::spawn(rt.run());
        ChannelHandle { shared, tx, status, rundown, events, preview }
    }

    pub fn id(&self) -> &str {
        &self.shared.id
    }

    /// Send a command and wait for its reply.
    pub async fn send(&self, cmd: Command) -> Result<String> {
        let (rtx, rrx) = oneshot::channel();
        self.tx.send(Request { cmd, reply: Some(rtx) }).await.map_err(|_| EngineError::ChannelGone)?;
        match rrx.await.map_err(|_| EngineError::ChannelGone)? {
            Ok(s) => Ok(s),
            Err(e) => Err(EngineError::Other(e)),
        }
    }

    pub fn status(&self) -> ChannelStatus {
        let mut st = self.status.borrow().clone();
        st.avatar = avatar::version(&self.read(|d| d.dir.clone()));
        st
    }

    pub fn status_watch(&self) -> watch::Receiver<ChannelStatus> {
        self.status.clone()
    }

    /// Upcoming rundown (on-air item excluded).
    pub fn rundown(&self) -> Arc<Rundown> {
        self.rundown.borrow().clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn preview(&self) -> PreviewFrame {
        self.preview.borrow().clone()
    }

    /// Read access to channel data.
    pub fn read<R>(&self, f: impl FnOnce(&ChannelData) -> R) -> R {
        let g = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
        f(&g)
    }

    /// Modify channel data, persist it and ask the actor to reload.
    pub async fn modify<R>(&self, f: impl FnOnce(&mut ChannelData) -> Result<R>) -> Result<R> {
        let r = {
            let mut g = self.shared.data.write().unwrap_or_else(|e| e.into_inner());
            let r = f(&mut g)?;
            g.save()?;
            r
        };
        self.send(Command::Reload).await?;
        Ok(r)
    }
}

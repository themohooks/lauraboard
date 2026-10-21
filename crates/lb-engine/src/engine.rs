//! Multi-channel engine: loads a project and runs one actor per channel.

use std::collections::BTreeMap;
use std::path::Path;

use lb_core::project::Project;

use crate::channel::ChannelHandle;
use crate::error::{EngineError, Result};

pub struct Engine {
    pub project_root: std::path::PathBuf,
    pub app: lb_core::config::AppConfig,
    channels: BTreeMap<String, ChannelHandle>,
}

impl Engine {
    /// Load the project and spawn all enabled channels (must be called
    /// inside a tokio runtime). Channels with `autostart` begin airing.
    pub fn start(root: &Path) -> Result<Self> {
        let project = Project::load(root)?;
        let mut channels = BTreeMap::new();
        for ch in project.channels {
            if !ch.config.enabled {
                tracing::info!(channel = %ch.id(), "channel disabled, not loaded");
                continue;
            }
            let id = ch.id().to_string();
            channels.insert(id, ChannelHandle::spawn(project.app.clone(), project.root.clone(), ch));
        }
        Ok(Engine { project_root: project.root, app: project.app, channels })
    }

    pub fn channels(&self) -> impl Iterator<Item = &ChannelHandle> {
        self.channels.values()
    }

    pub fn channel(&self, id: &str) -> Result<&ChannelHandle> {
        self.channels.get(id).ok_or_else(|| EngineError::UnknownChannel(id.to_string()))
    }

    /// `id`, or the single/first channel when `None`.
    pub fn pick(&self, id: Option<&str>) -> Result<&ChannelHandle> {
        match id {
            Some(id) => self.channel(id),
            None => self.channels.values().next().ok_or_else(|| EngineError::UnknownChannel("<none>".into())),
        }
    }

    /// Stop every channel gracefully.
    pub async fn shutdown(&self) {
        for ch in self.channels.values() {
            if let Err(e) = ch.send(crate::channel::Command::Stop).await {
                tracing::warn!(channel = %ch.id(), "stop failed: {e}");
            }
        }
    }
}

//! Multi-channel engine: loads a project and runs one actor per channel.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::RwLock;

use lb_core::config::ChannelConfig;
use lb_core::project::{APP_FILE, CHANNELS_DIR, ChannelData, Project, files};

use crate::channel::ChannelHandle;
use crate::error::{EngineError, Result};

pub struct Engine {
    pub project_root: std::path::PathBuf,
    pub app: lb_core::config::AppConfig,
    /// Channels can be added while running (see [`Engine::create_channel`]).
    channels: RwLock<BTreeMap<String, ChannelHandle>>,
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
        Ok(Engine { project_root: project.root, app: project.app, channels: RwLock::new(channels) })
    }

    fn map(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, ChannelHandle>> {
        self.channels.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn channels(&self) -> Vec<ChannelHandle> {
        self.map().values().cloned().collect()
    }

    pub fn channel(&self, id: &str) -> Result<ChannelHandle> {
        self.map().get(id).cloned().ok_or_else(|| EngineError::UnknownChannel(id.to_string()))
    }

    /// `id`, or the single/first channel when `None`.
    pub fn pick(&self, id: Option<&str>) -> Result<ChannelHandle> {
        match id {
            Some(id) => self.channel(id),
            None => self.map().values().next().cloned().ok_or_else(|| EngineError::UnknownChannel("<none>".into())),
        }
    }

    /// Create a channel directory and start its actor (must be called
    /// inside a tokio runtime). Settings are copied from `template` when
    /// given, without its outputs (they may hold stream keys of another
    /// channel). New channels do not start airing on their own.
    pub fn create_channel(&self, id: &str, name: &str, template: Option<&str>) -> Result<ChannelHandle> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(EngineError::InvalidConfig(format!("channel id '{id}': use latin letters, digits, '-' and '_'")));
        }
        if self.map().contains_key(id) {
            return Err(EngineError::InvalidConfig(format!("channel '{id}' already exists")));
        }
        let dir = self.project_root.join(CHANNELS_DIR).join(id);
        if dir.join("channel.toml").exists() {
            return Err(EngineError::InvalidConfig(format!("{} already exists (channel disabled?)", dir.display())));
        }
        let mut config = match template {
            Some(t) => self.channel(t)?.read(|d| d.config.clone()),
            None => ChannelConfig::new(id),
        };
        config.id = id.to_string();
        config.name = if name.trim().is_empty() { id.to_string() } else { name.trim().to_string() };
        config.enabled = true;
        config.autostart = false;
        config.outputs.clear();
        let data = ChannelData::new(dir, config);
        data.save()?;
        // an explicit channel list in laurabrod.toml must name it, or it is lost on restart
        if !self.app.channels.is_empty() {
            let path = self.project_root.join(APP_FILE);
            let mut app: lb_core::config::AppConfig = files::read_toml(&path)?;
            if !app.channels.iter().any(|c| c == id) {
                app.channels.push(id.to_string());
                files::write_toml(&path, &app)?;
            }
        }
        let handle = ChannelHandle::spawn(self.app.clone(), self.project_root.clone(), data);
        self.channels.write().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), handle.clone());
        tracing::info!(channel = %id, "channel created");
        Ok(handle)
    }

    /// Stop every channel gracefully.
    pub async fn shutdown(&self) {
        for ch in self.channels() {
            if let Err(e) = ch.send(crate::channel::Command::Stop).await {
                tracing::warn!(channel = %ch.id(), "stop failed: {e}");
            }
        }
    }
}

//! Project on disk: `laurabrod.toml` + `channels/<id>/…`.
//!
//! ```text
//! <root>/laurabrod.toml
//! <root>/channels/<id>/channel.toml
//!                      library.json
//!                      playlists/<playlist>.json
//!                      schedule.toml
//!                      rules.toml
//!                      graphics.toml
//!                      state/      (runtime, not edited by hand)
//!                      asrun/      (as-run logs, JSONL per day)
//! ```

pub mod files;
pub mod m3u;
pub mod paths;
pub mod rebase;

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, ChannelConfig};
use crate::error::{CoreError, Result};
use crate::graphics::GraphicsConfig;
use crate::media::Library;
use crate::playlist::Playlist;
use crate::rules::RuleSet;
use crate::schedule::ScheduleSet;

pub const APP_FILE: &str = "laurabrod.toml";
pub const CHANNELS_DIR: &str = "channels";

/// Everything a channel needs for planning and playout.
#[derive(Debug, Clone)]
pub struct ChannelData {
    pub dir: PathBuf,
    pub config: ChannelConfig,
    pub library: Library,
    pub playlists: Vec<Playlist>,
    pub schedules: ScheduleSet,
    pub rules: RuleSet,
    pub graphics: GraphicsConfig,
}

impl ChannelData {
    pub fn new(dir: PathBuf, config: ChannelConfig) -> Self {
        ChannelData {
            dir,
            config,
            library: Library::default(),
            playlists: vec![],
            schedules: ScheduleSet::default(),
            rules: RuleSet::default(),
            graphics: GraphicsConfig::default(),
        }
    }

    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub fn state_dir(&self) -> PathBuf {
        self.dir.join("state")
    }

    pub fn asrun_dir(&self) -> PathBuf {
        self.dir.join("asrun")
    }

    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn playlist_mut(&mut self, id: &str) -> Option<&mut Playlist> {
        self.playlists.iter_mut().find(|p| p.id == id)
    }

    /// Load a channel directory.
    pub fn load(dir: &Path) -> Result<Self> {
        let config: ChannelConfig = files::read_toml(&dir.join("channel.toml"))?;
        let mut library: Library = files::read_json_or_default(&dir.join("library.json"))?;
        library.reindex();
        let mut playlists = Vec::new();
        let pl_dir = dir.join("playlists");
        if pl_dir.is_dir() {
            let mut paths: Vec<PathBuf> = fs::read_dir(&pl_dir)
                .map_err(|e| CoreError::io(&pl_dir, e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect();
            paths.sort();
            for p in paths {
                playlists.push(files::read_json::<Playlist>(&p)?);
            }
        }
        Ok(ChannelData {
            dir: dir.to_path_buf(),
            config,
            library,
            playlists,
            schedules: files::read_toml_or_default(&dir.join("schedule.toml"))?,
            rules: files::read_toml_or_default(&dir.join("rules.toml"))?,
            graphics: files::read_toml_or_default(&dir.join("graphics.toml"))?,
        })
    }

    /// Save all channel files (removing playlist files that no longer exist).
    pub fn save(&self) -> Result<()> {
        files::write_toml(&self.dir.join("channel.toml"), &self.config)?;
        self.save_library()?;
        self.save_playlists()?;
        files::write_toml(&self.dir.join("schedule.toml"), &self.schedules)?;
        files::write_toml(&self.dir.join("rules.toml"), &self.rules)?;
        files::write_toml(&self.dir.join("graphics.toml"), &self.graphics)
    }

    pub fn save_library(&self) -> Result<()> {
        files::write_json(&self.dir.join("library.json"), &self.library)
    }

    pub fn save_playlists(&self) -> Result<()> {
        let pl_dir = self.dir.join("playlists");
        fs::create_dir_all(&pl_dir).map_err(|e| CoreError::io(&pl_dir, e))?;
        for p in &self.playlists {
            files::write_json(&pl_dir.join(format!("{}.json", p.id)), p)?;
        }
        if let Ok(rd) = fs::read_dir(&pl_dir) {
            for e in rd.flatten() {
                let path = e.path();
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
                if path.extension().is_some_and(|x| x == "json") && !self.playlists.iter().any(|p| p.id == stem) {
                    let _ = fs::remove_file(path);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub app: AppConfig,
    pub channels: Vec<ChannelData>,
}

impl Project {
    /// Load a project from its root directory.
    pub fn load(root: &Path) -> Result<Self> {
        let app_path = root.join(APP_FILE);
        if !app_path.exists() {
            return Err(CoreError::NotFound(format!("{} (run `laurabrod init`)", app_path.display())));
        }
        let app: AppConfig = files::read_toml(&app_path)?;
        let ch_root = root.join(CHANNELS_DIR);
        let ids: Vec<String> = if app.channels.is_empty() {
            let mut v: Vec<String> = fs::read_dir(&ch_root)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.path().join("channel.toml").exists())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            v.sort();
            v
        } else {
            app.channels.clone()
        };
        let mut channels = Vec::new();
        for id in ids {
            channels.push(ChannelData::load(&ch_root.join(&id))?);
        }
        Ok(Project { root: root.to_path_buf(), app, channels })
    }

    /// Create a new project skeleton with one channel.
    pub fn init(root: &Path, channel: &str) -> Result<Self> {
        let app_path = root.join(APP_FILE);
        if app_path.exists() {
            return Err(CoreError::Invalid(format!("{} already exists", app_path.display())));
        }
        let app = AppConfig::default();
        files::write_toml(&app_path, &app)?;
        let ch = ChannelData::new(root.join(CHANNELS_DIR).join(channel), ChannelConfig::new(channel));
        ch.save()?;
        Ok(Project { root: root.to_path_buf(), app, channels: vec![ch] })
    }

    pub fn save_app(&self) -> Result<()> {
        files::write_toml(&self.root.join(APP_FILE), &self.app)
    }

    pub fn channel(&self, id: &str) -> Option<&ChannelData> {
        self.channels.iter().find(|c| c.id() == id)
    }

    pub fn channel_mut(&mut self, id: &str) -> Option<&mut ChannelData> {
        self.channels.iter_mut().find(|c| c.id() == id)
    }

    /// The channel with this id, or the only/first one when `id` is None.
    pub fn pick_channel(&self, id: Option<&str>) -> Result<&ChannelData> {
        match id {
            Some(id) => self.channel(id).ok_or_else(|| CoreError::NotFound(format!("channel '{id}'"))),
            None => self.channels.first().ok_or_else(|| CoreError::NotFound("no channels in project".into())),
        }
    }

    pub fn resolve(&self, stored: &str) -> PathBuf {
        paths::resolve(&self.root, stored)
    }
}

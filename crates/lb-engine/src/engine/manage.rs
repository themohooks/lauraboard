//! Channel management while the engine runs: rename (display name and id),
//! duplicate with all content, delete.

use std::path::{Path, PathBuf};
use std::time::Duration;

use lb_core::project::ChannelData;

use super::Engine;
use crate::channel::{ChannelHandle, Command};
use crate::error::{EngineError, Result};

/// Folder (under the project root) that receives deleted channels.
pub const DELETED_DIR: &str = "deleted";

/// Runtime folders that belong to one channel instance and are not copied.
const NOT_COPIED: &[&str] = &["state", "asrun"];

impl Engine {
    /// Change the display name and, optionally, the id (folder name). The
    /// id can only change while the channel is off air.
    pub async fn rename_channel(&self, id: &str, new_id: &str, name: &str) -> Result<ChannelHandle> {
        let h = self.channel(id)?;
        let name = name.trim().to_string();
        let new_id = new_id.trim();
        if new_id.is_empty() || new_id == id {
            if !name.is_empty() {
                h.modify(move |d| {
                    d.config.name = name;
                    Ok(())
                })
                .await?;
            }
            return Ok(h);
        }
        let new_dir = self.check_new_id(new_id)?;
        if h.status().state != crate::channel::status::RunState::Stopped {
            return Err(EngineError::Other("stop the broadcast before changing the channel id".into()));
        }
        let old_dir = h.read(|d| d.dir.clone());
        let old_entry = self.project_root.join(lb_core::project::CHANNELS_DIR).join(id);
        let external = old_entry.join(lb_core::project::LOCATION_FILE).is_file();
        self.retire(&h).await;
        if let Err(e) = rename_dir(if external { &old_entry } else { &old_dir }, &new_dir) {
            // put the channel back as it was
            if let Ok(data) = ChannelData::load(&old_dir) {
                self.register(data);
            }
            return Err(e.into());
        }
        let mut data = ChannelData::load(&lb_core::project::channel_location(&new_dir))?;
        data.config.id = new_id.to_string();
        if !name.is_empty() {
            data.config.name = name;
        }
        data.rebase_paths(id, new_id);
        data.save()?;
        self.edit_channel_list(|l| {
            for c in l.iter_mut().filter(|c| *c == id) {
                *c = new_id.to_string();
            }
        })?;
        tracing::info!(channel = %new_id, old = %id, "channel renamed");
        Ok(self.register(data))
    }

    /// Copy a channel with its library, playlists, schedules, rules,
    /// graphics and own media files. Outputs are left empty (stream keys
    /// belong to the original) and the copy does not start on its own.
    pub async fn duplicate_channel(&self, src: &str, new_id: &str, name: &str) -> Result<ChannelHandle> {
        let h = self.channel(src)?;
        let new_dir = self.check_new_id(new_id)?;
        let (src_dir, record_dir) = h.read(|d| (d.dir.clone(), d.config.record.dir.clone()));
        let mut skip: Vec<PathBuf> = NOT_COPIED.iter().map(|n| src_dir.join(n)).collect();
        if !record_dir.trim().is_empty() {
            skip.push(src_dir.join(record_dir));
        }
        let from = src_dir.clone();
        let to = new_dir.clone();
        tokio::task::spawn_blocking(move || copy_dir(&from, &to, &skip)).await.map_err(|e| EngineError::Other(e.to_string()))??;
        let mut data = ChannelData::load(&new_dir)?;
        data.config.id = new_id.to_string();
        data.config.name = if name.trim().is_empty() { format!("{} (копия)", data.config.name) } else { name.trim().to_string() };
        data.config.autostart = false;
        data.config.outputs.clear();
        data.config.local_hls.enabled = false;
        // Older/external libraries may contain absolute paths to own files.
        data.for_each_path_mut(|path| {
            if let Ok(relative) = Path::new(path).strip_prefix(&src_dir) {
                *path = format!("channels/{new_id}/{}", relative.to_string_lossy().replace('\\', "/"));
            }
        });
        data.rebase_paths(src, new_id);
        data.save()?;
        self.edit_channel_list(|l| l.push(new_id.to_string()))?;
        tracing::info!(channel = %new_id, from = %src, "channel duplicated");
        Ok(self.register(data))
    }

    /// Stop and remove a channel. Its folder is moved to `deleted/` (or
    /// erased with `purge`). Returns where it was moved.
    pub async fn delete_channel(&self, id: &str, purge: bool) -> Result<Option<PathBuf>> {
        let h = self.channel(id)?;
        let dir = h.read(|d| d.dir.clone());
        let entry = self.project_root.join(lb_core::project::CHANNELS_DIR).join(id);
        let external = entry.join(lb_core::project::LOCATION_FILE).is_file();
        self.retire(&h).await;
        self.edit_channel_list(|l| l.retain(|c| c != id))?;
        let moved = if purge {
            remove_dir(&dir)?;
            None
        } else {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let bin =
                if external { dir.parent().unwrap_or(&self.project_root).join(DELETED_DIR) } else { self.project_root.join(DELETED_DIR) };
            std::fs::create_dir_all(&bin)?;
            let target = bin.join(format!("{id}-{stamp}"));
            rename_dir(&dir, &target)?;
            Some(target)
        };
        if external {
            std::fs::remove_file(entry.join(lb_core::project::LOCATION_FILE))?;
            std::fs::remove_dir(&entry)?;
        }
        tracing::info!(channel = %id, purge, "channel deleted");
        Ok(moved)
    }

    /// End the channel's actor and forget it.
    async fn retire(&self, h: &ChannelHandle) {
        if let Err(e) = h.send(Command::Shutdown).await {
            tracing::warn!(channel = %h.id(), "shutdown: {e}");
        }
        self.channels.write().unwrap_or_else(|e| e.into_inner()).remove(h.id());
    }
}

/// Rename with a few retries: on Windows a just-closed file (log, lock)
/// can keep the folder busy for a moment.
fn rename_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for _ in 0..10 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

fn remove_dir(dir: &Path) -> std::io::Result<()> {
    let mut last = None;
    for _ in 0..10 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("remove failed")))
}

/// Recursive copy of `from` into a new folder `to`, leaving out `skip`.
pub fn copy_dir(from: &Path, to: &Path, skip: &[PathBuf]) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let src = e.path();
        if skip.contains(&src) {
            continue;
        }
        let dst = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&src, &dst, skip)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn encoder_retry_on_a_stopped_channel_attempts_startup() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = lb_core::project::Project::init(dir.path(), "main").unwrap();
        let channel = project.channel_mut("main").unwrap();
        channel.config.autostart = false;
        channel.config.video.width = 0;
        channel.save().unwrap();
        let engine = Engine::start(dir.path()).unwrap();
        let error = engine.channel("main").unwrap().send(Command::RestartEncoder).await.unwrap_err();
        assert!(error.to_string().contains("validation failed"), "{error}");
        engine.shutdown().await;
    }

    #[tokio::test]
    async fn external_channel_survives_reload_rename_and_delete() {
        use lb_core::project::{Project, paths};
        let project = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let mut p = Project::init(project.path(), "main").unwrap();
        p.channel_mut("main").unwrap().config.autostart = false;
        p.channel("main").unwrap().save().unwrap();
        let engine = std::sync::Arc::new(Engine::start(project.path()).unwrap());
        assert!(engine.create_channel_at("missing", "", None, Some(&storage.path().join("missing-parent"))).is_err());
        std::fs::create_dir(storage.path().join("occupied")).unwrap();
        std::fs::write(storage.path().join("occupied/keep.txt"), b"keep").unwrap();
        assert!(engine.create_channel_at("occupied", "", None, Some(storage.path())).is_err());
        assert!(storage.path().join("occupied/keep.txt").exists());
        let channel = engine.create_channel_at("external", "External", None, Some(storage.path())).unwrap();
        let media = storage.path().join("external/media/a.mp4");
        std::fs::create_dir_all(media.parent().unwrap()).unwrap();
        std::fs::write(&media, b"media").unwrap();
        assert_eq!(channel.read(|d| d.dir.clone()), storage.path().join("external"));
        assert_eq!(paths::resolve(project.path(), "channels/external/media/a.mp4"), media);
        assert_eq!(paths::to_stored(project.path(), &media), "channels/external/media/a.mp4");
        assert!(Project::load(project.path()).unwrap().channel("external").is_some());
        engine.rename_channel("external", "renamed", "").await.unwrap();
        assert!(Project::load(project.path()).unwrap().channel("renamed").is_some());
        assert_eq!(paths::resolve(project.path(), "channels/renamed/media/a.mp4"), media);
        let bin = engine.delete_channel("renamed", false).await.unwrap().unwrap();
        assert!(bin.join("media/a.mp4").exists());
        assert!(!project.path().join("channels/renamed").exists());
        assert!(Project::load(project.path()).unwrap().channel("main").is_some());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let work = engine.clone();
        let archive = project.path().join("channel.zip");
        let destination = storage.path().to_path_buf();
        engine.jobs.spawn("test", "external import", None, move |ctl| async move {
            let result = async {
                work.clone().export_channel("main".into(), archive.clone(), Default::default(), ctl.clone()).await?;
                work.import_channel_at(archive, Some("imported".into()), None, Some(destination), ctl).await
            }
            .await;
            let _ = tx.send(result.clone());
            result
        });
        rx.await.unwrap().unwrap();
        assert!(storage.path().join("imported/channel.toml").is_file());
        assert!(Project::load(project.path()).unwrap().channel("imported").is_some());
        engine.shutdown().await;
    }

    #[test]
    fn copy_skips_runtime_folders() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("a");
        std::fs::create_dir_all(src.join("state")).unwrap();
        std::fs::create_dir_all(src.join("media")).unwrap();
        std::fs::write(src.join("channel.toml"), "x").unwrap();
        std::fs::write(src.join("state/s.json"), "x").unwrap();
        std::fs::write(src.join("media/1.mp4"), "x").unwrap();
        let dst = t.path().join("b");
        copy_dir(&src, &dst, &[src.join("state")]).unwrap();
        assert!(dst.join("channel.toml").exists());
        assert!(dst.join("media/1.mp4").exists());
        assert!(!dst.join("state").exists());
    }
}

//! Crash-recovery snapshot written on every transition and periodically.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use lb_core::WallTime;
use lb_core::planner::PlanState;
use lb_core::project::files;
use lb_core::rundown::RundownItem;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RuntimeState {
    /// Item on air when the snapshot was taken.
    #[serde(default)]
    pub on_air: Option<RundownItem>,
    /// When it actually started (wall clock) and from which media offset.
    #[serde(default)]
    pub started: Option<WallTime>,
    #[serde(default)]
    pub started_offset_ms: i64,
    #[serde(default)]
    pub plan: PlanState,
    /// Main items already aired in the on-air block.
    #[serde(default)]
    pub block_items_done: usize,
    /// Was the channel broadcasting (auto-resume after restart)?
    #[serde(default)]
    pub running: bool,
    /// Operator selected schedule.
    #[serde(default)]
    pub force_schedule: Option<String>,
    /// Manual mode active.
    #[serde(default)]
    pub manual: bool,
    #[serde(default)]
    pub saved_at: Option<WallTime>,
}

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("state.json")
}

pub fn load(state_dir: &Path) -> RuntimeState {
    let p = path(state_dir);
    match files::read_json_or_default::<RuntimeState>(&p) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("ignoring unreadable runtime state {}: {e}", p.display());
            RuntimeState::default()
        }
    }
}

pub fn save(state_dir: &Path, s: &RuntimeState) -> Result<()> {
    files::write_json(&path(state_dir), s)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_default_on_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), RuntimeState::default());
        let s = RuntimeState { running: true, block_items_done: 3, ..Default::default() };
        save(dir.path(), &s).unwrap();
        assert_eq!(load(dir.path()), s);
    }
}

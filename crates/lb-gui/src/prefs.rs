//! Interface preferences persisted between runs
//! (`<config dir>/laurabrod/gui.json`).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionMode {
    /// Follow the OS "reduce motion" setting.
    #[default]
    System,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    pub theme: ThemeMode,
    /// UI scale on top of the system DPI (1.0 = 100 %).
    pub scale: f32,
    pub compact: bool,
    pub motion: MotionMode,
    pub nav_expanded: bool,
    /// Last opened section (stable id, see the navigation model).
    pub section: String,
    /// Splitter positions by id.
    pub panels: HashMap<String, f32>,
    pub library_grid: bool,
}

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs {
            theme: ThemeMode::System,
            scale: 1.0,
            compact: false,
            motion: MotionMode::System,
            nav_expanded: true,
            section: "home".into(),
            panels: HashMap::new(),
            library_grid: false,
        }
    }
}

impl UiPrefs {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("laurabrod").join("gui.json"))
    }

    /// Load saved preferences; anything missing or unreadable falls back to defaults.
    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<UiPrefs>(&b).ok()).unwrap_or_default().sanitized()
    }

    /// Save atomically (temp file + rename). Errors are ignored: preferences are not critical.
    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        let Ok(json) = serde_json::to_vec_pretty(self) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    fn sanitized(mut self) -> Self {
        if !self.scale.is_finite() {
            self.scale = 1.0;
        }
        self.scale = self.scale.clamp(0.8, 2.0);
        self
    }

    /// Whether decorative animations should play.
    pub fn animations(&self) -> bool {
        match self.motion {
            MotionMode::On => true,
            MotionMode::Off => false,
            MotionMode::System => !crate::sysprefs::reduce_motion(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        let p: UiPrefs = serde_json::from_str(r#"{"theme":"dark","scale":9}"#).unwrap();
        let p = p.sanitized();
        assert_eq!(p.theme, ThemeMode::Dark);
        assert_eq!(p.scale, 2.0);
        assert!(p.nav_expanded);
        assert_eq!(p.section, "home");
    }
}

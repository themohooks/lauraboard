//! Interface preferences persisted between runs
//! (`<config dir>/lauraboard/gui.json`).

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

/// Which events raise a system notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyPrefs {
    pub enabled: bool,
    /// Encoder stopped / restored, connection to the server lost.
    pub errors: bool,
    /// Items skipped or cut short (missing file, decoder failure).
    pub skipped: bool,
    /// Broadcast points (RTMP, SRT…) unreachable.
    pub outputs: bool,
    /// Little free space left on the recording disk.
    pub disk: bool,
    /// Air started / stopped, manual mode, schedule switched.
    pub air: bool,
    /// Every new programme on air (not interstitials).
    pub programs: bool,
    /// Only while the window is hidden or minimised.
    pub only_hidden: bool,
}

impl Default for NotifyPrefs {
    fn default() -> Self {
        NotifyPrefs {
            enabled: true,
            errors: true,
            skipped: true,
            outputs: true,
            disk: true,
            air: true,
            programs: false,
            only_hidden: false,
        }
    }
}

/// Update checks (see `crate::update`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatePrefs {
    /// Look for a new version quietly once a day.
    pub auto: bool,
    /// A version the operator chose to skip (no mark in the app bar).
    pub skipped: String,
    /// Unix time of the last successful check.
    pub last_check: Option<i64>,
}

impl Default for UpdatePrefs {
    fn default() -> Self {
        UpdatePrefs { auto: true, skipped: String::new(), last_check: None }
    }
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
    /// The close button hides the window into the notification area
    /// (playout keeps running); quit from the tray menu.
    pub close_to_tray: bool,
    /// System notifications (Windows / Linux / macOS).
    pub notify: NotifyPrefs,
    /// Plate designs saved by the user ("my styles").
    pub plate_styles: Vec<lb_core::graphics::PlateStyle>,
    pub updates: UpdatePrefs,
    /// Written to disk on change (off for headless tools).
    #[serde(skip)]
    pub persist: bool,
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
            close_to_tray: true,
            notify: NotifyPrefs::default(),
            plate_styles: Vec::new(),
            updates: UpdatePrefs::default(),
            persist: true,
        }
    }
}

impl UiPrefs {
    pub fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("lauraboard").join("gui.json"))
    }

    /// Load saved preferences; anything missing or unreadable falls back to defaults.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<UiPrefs>(&b).ok())
            .unwrap_or_default()
            .sanitized()
    }

    /// Save atomically (temp file + rename). Errors are ignored: preferences are not critical.
    pub fn save(&self) {
        if !self.persist {
            return;
        }
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
        assert!(p.close_to_tray);
        assert!(p.updates.auto);
    }
}

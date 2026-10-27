//! Video editor: changes rendered into a new file of the library — trim,
//! hidden areas (a logo blurred, pixelated, painted over), crop, sound and
//! fades. Coordinates are fractions of the source picture (0 … 1), times
//! are milliseconds of the source file.

use serde::{Deserialize, Serialize};

use super::display::Crop;
use crate::Ms;

/// How a region of the picture is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RegionMode {
    /// Soft blur (good for logos on a busy picture).
    #[default]
    Blur,
    /// Large squares.
    Pixelate,
    /// Solid colour.
    Fill,
    /// Fill from the surrounding pixels (ffmpeg `delogo`): almost invisible
    /// on calm backgrounds.
    Delogo,
}

/// A rectangle hidden on the picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub mode: RegionMode,
    /// 0 … 1: blur radius / square size.
    #[serde(default = "half")]
    pub strength: f32,
    /// `Fill` colour (`#RRGGBB`).
    #[serde(default = "black")]
    pub color: String,
    /// Shown only from / until this time of the source; `None` = whole clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_ms: Option<Ms>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_ms: Option<Ms>,
}

fn half() -> f32 {
    0.5
}
fn black() -> String {
    "#000000".into()
}

impl Region {
    /// The rectangle kept inside the picture, at least 1 % in size.
    pub fn clamped(&self) -> (f32, f32, f32, f32) {
        let x = self.x.clamp(0.0, 0.99);
        let y = self.y.clamp(0.0, 0.99);
        let w = self.w.clamp(0.01, 1.0 - x);
        let h = self.h.clamp(0.01, 1.0 - y);
        (x, y, w, h)
    }
}

/// Everything the editor changes. Default = the file as it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EditSpec {
    /// Start of the kept part (ms of the source).
    #[serde(default)]
    pub start_ms: Ms,
    /// End of the kept part; `None` = to the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<Ms>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<Region>,
    /// Edges cut off, percent of the source (applied after the regions).
    #[serde(default, skip_serializing_if = "Crop::is_empty")]
    pub crop: Crop,
    /// Remove the sound.
    #[serde(default)]
    pub mute: bool,
    /// Gain in dB.
    #[serde(default)]
    pub volume_db: f32,
    /// Fades from / to black and silence.
    #[serde(default)]
    pub fade_in_ms: Ms,
    #[serde(default)]
    pub fade_out_ms: Ms,
}

impl EditSpec {
    pub fn is_empty(&self) -> bool {
        *self == EditSpec::default()
    }

    /// The kept part `(start, end)` within a source of `duration` ms.
    pub fn range(&self, duration: Ms) -> (Ms, Ms) {
        let end = self.end_ms.filter(|e| *e > 0).unwrap_or(duration);
        let end = if duration > 0 { end.min(duration) } else { end };
        let start = self.start_ms.clamp(0, end.max(0));
        (start, end)
    }

    /// Problems that make rendering pointless.
    pub fn validate(&self, duration: Ms) -> Result<(), String> {
        let (s, e) = self.range(duration);
        if e - s < 100 {
            return Err("the kept part is too short".into());
        }
        if self.regions.len() > 16 {
            return Err("too many hidden areas (16 at most)".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_and_validation() {
        let e = EditSpec { start_ms: 2_000, end_ms: Some(50_000), ..Default::default() };
        assert_eq!(e.range(10_000), (2_000, 10_000));
        assert!(e.validate(10_000).is_ok());
        let bad = EditSpec { start_ms: 9_990, ..Default::default() };
        assert!(bad.validate(10_000).is_err());
        assert!(EditSpec::default().is_empty());
    }

    #[test]
    fn region_defaults_and_clamp() {
        let r: Region = serde_json::from_str(r#"{"x":0.9,"y":-1,"w":0.5,"h":0}"#).unwrap();
        assert_eq!(r.mode, RegionMode::Blur);
        assert_eq!(r.color, "#000000");
        let (x, y, w, h) = r.clamped();
        assert_eq!((x, y), (0.9, 0.0));
        assert!((w - 0.1).abs() < 1e-6 && (h - 0.01).abs() < 1e-6);
    }
}

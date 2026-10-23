//! How a media item is shown on air: scaling override, crop, picture
//! adjustments and clean-up filters. Everything defaults to "as is"; the
//! channel's video format decides the rest.

use serde::{Deserialize, Serialize};

use crate::config::ScaleMode;

/// Rotation applied before scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Rotate {
    #[default]
    None,
    /// 90° clockwise.
    Cw90,
    R180,
    /// 90° counter-clockwise.
    Ccw90,
}

/// Cut off the edges of the picture, in percent of the source size
/// (burnt-in black bars, a broadcaster's logo, VHS noise at the bottom).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Crop {
    #[serde(default)]
    pub left: f32,
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub bottom: f32,
}

impl Crop {
    pub fn is_empty(&self) -> bool {
        [self.left, self.top, self.right, self.bottom].iter().all(|v| *v <= 0.0)
    }

    /// Values limited so that at least 10 % of each side remains.
    pub fn clamped(&self) -> Crop {
        let c = |v: f32| v.clamp(0.0, 45.0);
        Crop { left: c(self.left), top: c(self.top), right: c(self.right), bottom: c(self.bottom) }
    }
}

/// Per-item display settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaDisplay {
    /// Fit / fill / stretch / blurred background; `None` = channel setting.
    /// `stretch` fills a 16:9 screen with a 4:3 clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<ScaleMode>,
    /// Treat the source as this aspect ratio (`4:3`, `16:9`, `2.35`), for
    /// files with wrong or missing aspect flags (anamorphic DVD rips).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<String>,
    #[serde(default, skip_serializing_if = "Crop::is_empty")]
    pub crop: Crop,
    /// Remove interlacing combs (old TV recordings).
    #[serde(default)]
    pub deinterlace: bool,
    #[serde(default)]
    pub rotate: Rotate,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    /// -1 … 1, 0 = unchanged.
    #[serde(default)]
    pub brightness: f32,
    /// 0 … 2, 1 = unchanged.
    #[serde(default = "one")]
    pub contrast: f32,
    /// 0 (black and white) … 3, 1 = unchanged.
    #[serde(default = "one")]
    pub saturation: f32,
    /// 0.1 … 3, 1 = unchanged.
    #[serde(default = "one")]
    pub gamma: f32,
    /// 0 … 2, 0 = off.
    #[serde(default)]
    pub sharpen: f32,
    /// 0 … 10, 0 = off.
    #[serde(default)]
    pub denoise: f32,
}

fn one() -> f32 {
    1.0
}

impl Default for MediaDisplay {
    fn default() -> Self {
        MediaDisplay {
            scale: None,
            aspect: None,
            crop: Crop::default(),
            deinterlace: false,
            rotate: Rotate::None,
            flip_h: false,
            flip_v: false,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            sharpen: 0.0,
            denoise: 0.0,
        }
    }
}

impl MediaDisplay {
    pub fn is_default(&self) -> bool {
        *self == MediaDisplay::default()
    }

    /// Colour correction differs from neutral.
    pub fn has_eq(&self) -> bool {
        self.brightness.abs() > 0.001
            || (self.contrast - 1.0).abs() > 0.001
            || (self.saturation - 1.0).abs() > 0.001
            || (self.gamma - 1.0).abs() > 0.001
    }

    /// Aspect ratio from `aspect` (`4:3`, `16/9`, `1.85`), if valid.
    pub fn aspect_ratio(&self) -> Option<f64> {
        parse_aspect(self.aspect.as_deref()?)
    }
}

/// `4:3`, `16/9`, `1.85` → ratio.
pub fn parse_aspect(s: &str) -> Option<f64> {
    let s = s.trim();
    let r = match s.split_once([':', '/']) {
        Some((a, b)) => a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
        None => s.replace(',', ".").parse().ok()?,
    };
    (r.is_finite() && (0.2..=5.0).contains(&r)).then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_omitted_and_partial_parses() {
        assert!(MediaDisplay::default().is_default());
        let d: MediaDisplay = serde_json::from_str(r#"{"scale":"stretch","saturation":0}"#).unwrap();
        assert_eq!(d.scale, Some(ScaleMode::Stretch));
        assert_eq!(d.contrast, 1.0);
        assert!(d.has_eq());
    }

    #[test]
    fn aspect_forms() {
        assert_eq!(parse_aspect("4:3").map(|r| (r * 100.0).round()), Some(133.0));
        assert_eq!(parse_aspect("16/9").map(|r| (r * 100.0).round()), Some(178.0));
        assert_eq!(parse_aspect("2,35"), Some(2.35));
        assert_eq!(parse_aspect("abc"), None);
        assert_eq!(parse_aspect("1:0"), None);
    }
}

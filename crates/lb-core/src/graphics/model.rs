//! Graphic element definitions: placement, text style, element kinds, animation.

use serde::{Deserialize, Serialize};

use crate::Dur;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Placement {
    #[serde(default)]
    pub anchor: Anchor,
    /// Margin from the anchored edges, in output pixels.
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// Size in pixels; `None` = natural size. For images one dimension keeps
    /// the aspect ratio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    /// Font family name (system) or path to a .ttf/.otf file.
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default = "default_size")]
    pub size: f32,
    #[serde(default = "white")]
    pub color: String,
    #[serde(default)]
    pub outline_color: Option<String>,
    #[serde(default)]
    pub outline_width: f32,
    #[serde(default)]
    pub shadow_color: Option<String>,
    #[serde(default = "default_shadow")]
    pub shadow_offset: (f32, f32),
    /// Background plate colour behind the text.
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default = "default_padding")]
    pub padding: f32,
    #[serde(default)]
    pub corner_radius: f32,
}

fn default_size() -> f32 {
    36.0
}
fn white() -> String {
    "#FFFFFF".into()
}
fn default_shadow() -> (f32, f32) {
    (2.0, 2.0)
}
fn default_padding() -> f32 {
    12.0
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font: None,
            size: default_size(),
            color: white(),
            outline_color: None,
            outline_width: 0.0,
            shadow_color: None,
            shadow_offset: default_shadow(),
            background: None,
            padding: default_padding(),
            corner_radius: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CountdownTarget {
    /// Remaining time of the current item.
    #[default]
    ItemRemaining,
    /// Time until the next block / programme.
    NextProgram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementKind {
    /// PNG/JPEG/WebP image (alpha supported).
    Image { path: String },
    /// Text plate / title. `text` supports `{placeholders}`.
    Text {
        text: String,
        #[serde(default)]
        style: TextStyle,
    },
    /// Scrolling ticker.
    Ticker {
        text: String,
        #[serde(default)]
        style: TextStyle,
        /// Pixels per second.
        #[serde(default = "default_speed")]
        speed: f32,
        /// Ticker band width (default: full width minus margins).
        #[serde(default)]
        width: Option<f32>,
    },
    /// Clock / date, `format` is strftime (`%H:%M`, `%d.%m.%Y`).
    Clock {
        #[serde(default = "default_clock")]
        format: String,
        #[serde(default)]
        style: TextStyle,
    },
    Countdown {
        #[serde(default)]
        target: CountdownTarget,
        #[serde(default)]
        style: TextStyle,
        #[serde(default)]
        prefix: String,
    },
    /// Solid (rounded) rectangle, e.g. lower-third background.
    Rect {
        color: String,
        width: f32,
        height: f32,
        #[serde(default)]
        radius: f32,
    },
    /// Composition: children are placed relative to this element.
    Group { children: Vec<String> },
}

fn default_speed() -> f32 {
    120.0
}
fn default_clock() -> String {
    "%H:%M".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnimKind {
    #[default]
    None,
    Fade,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Animation {
    #[serde(default, rename = "in")]
    pub anim_in: AnimKind,
    #[serde(default, rename = "out")]
    pub anim_out: AnimKind,
    #[serde(default = "default_anim")]
    pub duration: Dur,
}

fn default_anim() -> Dur {
    Dur(500)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub kind: ElementKind,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub layer: i32,
    #[serde(default)]
    pub animation: Animation,
    /// Design profiles this element belongs to (empty = all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,
}

fn one() -> f32 {
    1.0
}

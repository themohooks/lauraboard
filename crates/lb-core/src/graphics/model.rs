//! Graphic element definitions: placement, text style, element kinds, animation.

use serde::{Deserialize, Serialize};

use super::program::{FillDirection, ProgressTarget, TimeFormat};
use super::shape::{PlateField, ShapeStyle, TextAlign, plate_color, plate_pad_x, plate_pad_y, plate_spacing};
use crate::Dur;
use crate::media::MediaKind;

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
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
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

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn track_color() -> String {
    "#FFFFFF40".into()
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
            bold: false,
            italic: false,
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
    /// Time left in the programme on air (its block), without the
    /// excluded content kinds.
    ProgramRemaining,
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
        /// Content kinds not counted (programme countdown).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        exclude: Vec<MediaKind>,
        #[serde(default, skip_serializing_if = "is_default")]
        format: TimeFormat,
    },
    /// Progress bar of the programme (or the item) on air.
    Progress {
        width: f32,
        height: f32,
        /// Filled part.
        color: String,
        /// Second colour of the filled part: a gradient along the bar.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gradient: Option<String>,
        /// The empty bar behind it.
        #[serde(default = "track_color")]
        track: String,
        #[serde(default)]
        radius: f32,
        #[serde(default)]
        target: ProgressTarget,
        /// Content kinds not counted.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        exclude: Vec<MediaKind>,
        #[serde(default, skip_serializing_if = "is_default")]
        direction: FillDirection,
        /// Design of the bar (border, shadow, slant, texture…).
        #[serde(default, skip_serializing_if = "ShapeStyle::is_plain")]
        shape: ShapeStyle,
    },
    /// Rectangle (gradient, border, slant, shadow… in `shape`).
    Rect {
        color: String,
        width: f32,
        height: f32,
        #[serde(default)]
        radius: f32,
        #[serde(default, skip_serializing_if = "ShapeStyle::is_plain")]
        shape: ShapeStyle,
    },
    /// Plate (lower third): a designed background with lines of text that
    /// it grows to fit.
    Plate {
        #[serde(default)]
        fields: Vec<PlateField>,
        #[serde(default = "plate_color")]
        color: String,
        #[serde(default)]
        radius: f32,
        #[serde(default, skip_serializing_if = "ShapeStyle::is_plain")]
        shape: ShapeStyle,
        /// Fixed width; `None` = as wide as the longest line.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<f32>,
        #[serde(default)]
        min_width: f32,
        #[serde(default = "plate_pad_x")]
        padding_x: f32,
        #[serde(default = "plate_pad_y")]
        padding_y: f32,
        /// Gap between the lines.
        #[serde(default = "plate_spacing")]
        spacing: f32,
        #[serde(default)]
        align: TextAlign,
    },
    /// Composition: children are placed relative to this element.
    Group { children: Vec<String> },
}

impl ElementKind {
    /// Text style of text-drawing kinds.
    pub fn style(&self) -> Option<&TextStyle> {
        match self {
            ElementKind::Text { style, .. }
            | ElementKind::Ticker { style, .. }
            | ElementKind::Clock { style, .. }
            | ElementKind::Countdown { style, .. } => Some(style),
            _ => None,
        }
    }

    /// Design of shape kinds (rectangle, plate, progress bar).
    pub fn shape(&self) -> Option<&ShapeStyle> {
        match self {
            ElementKind::Rect { shape, .. } | ElementKind::Plate { shape, .. } | ElementKind::Progress { shape, .. } => Some(shape),
            _ => None,
        }
    }

    pub fn shape_mut(&mut self) -> Option<&mut ShapeStyle> {
        match self {
            ElementKind::Rect { shape, .. } | ElementKind::Plate { shape, .. } | ElementKind::Progress { shape, .. } => Some(shape),
            _ => None,
        }
    }

    /// Every text style of the element (a plate has one per field).
    pub fn styles_mut(&mut self) -> Vec<&mut TextStyle> {
        match self {
            ElementKind::Plate { fields, .. } => fields.iter_mut().map(|f| &mut f.style).collect(),
            other => other.style_mut().into_iter().collect(),
        }
    }

    pub fn style_mut(&mut self) -> Option<&mut TextStyle> {
        match self {
            ElementKind::Text { style, .. }
            | ElementKind::Ticker { style, .. }
            | ElementKind::Clock { style, .. }
            | ElementKind::Countdown { style, .. } => Some(style),
            _ => None,
        }
    }
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
    /// Slides: the element moves in this direction across its own size.
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    /// Short move (a fraction of the size) with a fade.
    FadeLeft,
    FadeRight,
    FadeUp,
    FadeDown,
    /// Grows from small / shrinks from large, with a fade.
    ZoomIn,
    ZoomOut,
    /// Grows with a slight overshoot.
    Pop,
    /// Reveal from one edge (the visible part grows in this direction).
    WipeLeft,
    WipeRight,
    WipeUp,
    WipeDown,
    /// Unfolds from the middle across / up and down.
    StretchX,
    StretchY,
    /// Slides up from below / drops from above with a bounce.
    BounceUp,
    DropDown,
    /// Turns into place from a quarter turn.
    Spin,
    /// Flickers a few times before settling.
    Flicker,
}

impl AnimKind {
    pub const ALL: [AnimKind; 23] = [
        AnimKind::None,
        AnimKind::Fade,
        AnimKind::SlideLeft,
        AnimKind::SlideRight,
        AnimKind::SlideUp,
        AnimKind::SlideDown,
        AnimKind::FadeLeft,
        AnimKind::FadeRight,
        AnimKind::FadeUp,
        AnimKind::FadeDown,
        AnimKind::ZoomIn,
        AnimKind::ZoomOut,
        AnimKind::Pop,
        AnimKind::WipeLeft,
        AnimKind::WipeRight,
        AnimKind::WipeUp,
        AnimKind::WipeDown,
        AnimKind::StretchX,
        AnimKind::StretchY,
        AnimKind::BounceUp,
        AnimKind::DropDown,
        AnimKind::Spin,
        AnimKind::Flicker,
    ];
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

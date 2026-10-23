//! Design of shapes (rectangles and plate backgrounds) and the text fields
//! of plates.

use serde::{Deserialize, Serialize};

use super::model::TextStyle;

/// Side of a shape (accent bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Top,
    Right,
    Bottom,
}

/// A coloured bar along one side (e.g. the channel colour on a lower third).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Accent {
    #[serde(default)]
    pub side: Side,
    /// Thickness in pixels.
    #[serde(default = "accent_size")]
    pub size: f32,
    pub color: String,
}

fn accent_size() -> f32 {
    8.0
}

/// Everything beyond a flat colour: gradient, border, individual corners,
/// slant, soft shadow, accent bar, texture. All optional; the default is a
/// plain shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ShapeStyle {
    /// Second colour: a linear gradient from the main colour to this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient: Option<String>,
    /// Gradient direction in degrees (0 = left → right, 90 = top → bottom).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gradient_angle: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_color: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub border_width: f32,
    /// Radii of the corners: top-left, top-right, bottom-right, bottom-left
    /// (overrides the single radius).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corners: Option<[f32; 4]>,
    /// Slant in degrees (a parallelogram; positive leans right).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skew: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_color: Option<String>,
    /// Shadow softness in pixels (0 = hard edge).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shadow_blur: f32,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub shadow_offset: (f32, f32),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<Accent>,
    /// Picture drawn inside the shape (stretched to cover it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn is_origin(v: &(f32, f32)) -> bool {
    v.0 == 0.0 && v.1 == 0.0
}

impl ShapeStyle {
    pub fn is_plain(&self) -> bool {
        *self == ShapeStyle::default()
    }

    /// Extra room around the shape for its shadow and slant:
    /// (left, top, right, bottom).
    pub fn margins(&self, h: f32) -> (f32, f32, f32, f32) {
        let slant = (self.skew.to_radians().tan() * h).abs();
        let (mut l, mut t, mut r, mut b) = (slant / 2.0, 0.0f32, slant / 2.0, 0.0f32);
        if self.shadow_color.is_some() {
            let spread = self.shadow_blur.max(0.0) * 2.0;
            let (dx, dy) = self.shadow_offset;
            l = l.max(spread - dx);
            r = r.max(spread + dx);
            t = t.max(spread - dy);
            b = b.max(spread + dy);
        }
        (l.max(0.0).ceil(), t.max(0.0).ceil(), r.max(0.0).ceil(), b.max(0.0).ceil())
    }
}

/// Horizontal alignment of plate fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// One line of text on a plate ("Имя", "Должность", …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlateField {
    /// Name of the field for the operator.
    #[serde(default)]
    pub label: String,
    /// Text with `{variables}`.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub style: TextStyle,
}

impl PlateField {
    pub fn new(label: &str, text: &str, size: f32, color: &str) -> Self {
        PlateField {
            label: label.into(),
            text: text.into(),
            style: TextStyle { size, color: color.into(), shadow_color: None, ..Default::default() },
        }
    }
}

pub(super) fn plate_color() -> String {
    "#101828E0".into()
}
pub(super) fn plate_pad_x() -> f32 {
    24.0
}
pub(super) fn plate_pad_y() -> f32 {
    14.0
}
pub(super) fn plate_spacing() -> f32 {
    4.0
}

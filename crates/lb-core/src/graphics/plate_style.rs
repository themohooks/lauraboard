//! A plate design saved under a name ("my styles"): everything about the
//! look of a plate — background, shape, padding, layout and the text style
//! of each line — but not the lines' text, the size or the picture. Applied
//! to another plate it redesigns it and keeps what that plate says.

use serde::{Deserialize, Serialize};

use super::model::{ElementKind, TextStyle};
use super::shape::{PlateLayout, ShapeStyle, TextAlign, VAlign};

/// Look of one line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineStyle {
    #[serde(default)]
    pub style: TextStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<TextAlign>,
    #[serde(default)]
    pub gap_before: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlateStyle {
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub radius: f32,
    #[serde(default)]
    pub shape: ShapeStyle,
    #[serde(default)]
    pub min_width: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub spacing: f32,
    #[serde(default)]
    pub align: TextAlign,
    #[serde(default)]
    pub layout: PlateLayout,
    #[serde(default)]
    pub valign: VAlign,
    /// By line; a plate with more lines repeats the last.
    #[serde(default)]
    pub lines: Vec<LineStyle>,
}

impl PlateStyle {
    /// The design of plate `k` (`None`: not a plate).
    pub fn capture(name: &str, k: &ElementKind) -> Option<PlateStyle> {
        let ElementKind::Plate { fields, color, radius, shape, min_width, padding_x, padding_y, spacing, align, layout, valign, .. } = k else {
            return None;
        };
        Some(PlateStyle {
            name: name.into(),
            color: color.clone(),
            radius: *radius,
            shape: shape.clone(),
            min_width: *min_width,
            padding_x: *padding_x,
            padding_y: *padding_y,
            spacing: *spacing,
            align: *align,
            layout: *layout,
            valign: *valign,
            lines: fields.iter().map(|f| LineStyle { style: f.style.clone(), align: f.align, gap_before: f.gap_before }).collect(),
        })
    }

    /// Redesign plate `k`; its lines keep their text. False: not a plate.
    pub fn apply(&self, k: &mut ElementKind) -> bool {
        let ElementKind::Plate { fields, color, radius, shape, min_width, padding_x, padding_y, spacing, align, layout, valign, .. } = k else {
            return false;
        };
        *color = self.color.clone();
        *radius = self.radius;
        *shape = self.shape.clone();
        *min_width = self.min_width;
        *padding_x = self.padding_x;
        *padding_y = self.padding_y;
        *spacing = self.spacing;
        *align = self.align;
        *layout = self.layout;
        *valign = self.valign;
        for (i, f) in fields.iter_mut().enumerate() {
            if let Some(l) = self.lines.get(i).or(self.lines.last()) {
                f.style = l.style.clone();
                f.align = l.align;
                f.gap_before = l.gap_before;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::PlateField;

    fn plate(texts: &[&str], color: &str) -> ElementKind {
        ElementKind::Plate {
            fields: texts.iter().enumerate().map(|(i, t)| PlateField::new("", t, 40.0 - i as f32 * 10.0, "#FFFFFF")).collect(),
            color: color.into(),
            radius: 0.0,
            shape: ShapeStyle::default(),
            width: Some(800.0),
            min_width: 0.0,
            padding_x: 24.0,
            padding_y: 14.0,
            spacing: 4.0,
            align: TextAlign::Left,
            layout: PlateLayout::Column,
            valign: VAlign::Center,
            fit: false,
            icon: None,
        }
    }

    #[test]
    fn a_saved_style_redesigns_another_plate_keeping_its_text() {
        let mut a = plate(&["Имя", "Должность"], "#C8102E");
        if let ElementKind::Plate { radius, shape, fields, .. } = &mut a {
            *radius = 12.0;
            shape.skew = -10.0;
            fields[1].style.italic = true;
        }
        let st = PlateStyle::capture("Мой", &a).unwrap();
        let mut b = plate(&["Иван", "Гость", "Москва"], "#000000");
        assert!(st.apply(&mut b));
        let ElementKind::Plate { fields, color, radius, shape, width, .. } = &b else { unreachable!() };
        assert_eq!((color.as_str(), *radius, shape.skew, *width), ("#C8102E", 12.0, -10.0, Some(800.0)));
        assert_eq!(fields.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["Иван", "Гость", "Москва"]);
        // the third line repeats the last saved one
        assert!(fields[1].style.italic && fields[2].style.italic && !fields[0].style.italic);
        assert!(!st.apply(&mut ElementKind::Group { children: vec![] }));
    }
}

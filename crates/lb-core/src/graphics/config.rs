//! Layers, display rules and the graphics configuration file.

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

use super::model::{Element, ElementKind};
use crate::Dur;
use crate::rules::Conditions;

/// A logical layer: a named group of elements drawn at one height
/// (`id`, higher on top) — "Логотип и часы", "Плашки", "Сцены"…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: i32,
    #[serde(default)]
    pub name: String,
    /// Only the highest priority element is shown on an exclusive layer.
    #[serde(default)]
    pub exclusive: bool,
    /// Switched off: its elements do not go on air by rules (a manual
    /// "show" still does).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

impl Default for Layer {
    fn default() -> Self {
        Layer { id: 0, name: String::new(), exclusive: false, enabled: true }
    }
}

impl Layer {
    pub fn new(id: i32, name: impl Into<String>) -> Self {
        Layer { id, name: name.into(), ..Default::default() }
    }
}

fn is_true(v: &bool) -> bool {
    *v
}

/// Layers offered for new elements, by kind: (id, name).
pub const LAYER_PRESETS: [(i32, &str); 5] =
    [(10, "Фон и подложки"), (20, "Плашки и титры"), (30, "Бегущие строки"), (40, "Логотип, часы и таймеры"), (50, "Сцены")];

/// Preset layer for a new element of this kind.
pub fn preset_layer(kind: &ElementKind) -> (i32, &'static str) {
    let i = match kind {
        ElementKind::Rect { .. } | ElementKind::Group { .. } => 0,
        ElementKind::Plate { .. } | ElementKind::Text { .. } => 1,
        ElementKind::Ticker { .. } => 2,
        ElementKind::Image { .. } | ElementKind::Clock { .. } | ElementKind::Countdown { .. } | ElementKind::Progress { .. } => 3,
    };
    LAYER_PRESETS[i]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GfxTiming {
    /// Daily absolute times; visible for `duration` from each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub at: Vec<NaiveTime>,
    /// Show this long after the item starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_start: Option<Dur>,
    /// Hide this long before the item ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_end: Option<Dur>,
    /// Visible duration (open-ended when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Dur>,
    /// Show only during the last part of the item (e.g. "next up" plates).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Dur>,
    /// Repeat the (after_start, duration) window with this period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<Dur>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphicRule {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub element: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<i32>,
    #[serde(default)]
    pub timing: GfxTiming,
    #[serde(default)]
    pub conditions: Conditions,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,
    /// Text of the element while this rule shows it, `{variables}` allowed:
    /// the lines of a plate by position, the text of a text element or a
    /// ticker as the first entry. An empty entry keeps the element's own
    /// text. One plate design then serves many rules (news, weather, the
    /// presenter of the current show…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<String>,
    /// Do not show the element when all its text comes out empty (say, a
    /// `{presenter}` plate on an item without a presenter).
    #[serde(default, skip_serializing_if = "is_false")]
    pub hide_if_empty: bool,
}

fn yes() -> bool {
    true
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl GraphicRule {
    /// `el` with this rule's texts applied (`None`: nothing to change).
    pub fn apply_texts(&self, el: &Element) -> Option<Element> {
        let pick = |i: usize| self.texts.get(i).filter(|t| !t.trim().is_empty());
        if (0..self.texts.len()).all(|i| pick(i).is_none()) {
            return None;
        }
        let mut el = el.clone();
        match &mut el.kind {
            ElementKind::Plate { fields, .. } => {
                for (i, f) in fields.iter_mut().enumerate() {
                    if let Some(t) = pick(i) {
                        f.text = t.clone();
                    }
                }
            }
            ElementKind::Text { text, .. } | ElementKind::Ticker { text, .. } => {
                if let Some(t) = pick(0) {
                    *text = t.clone();
                }
            }
            _ => return None,
        }
        Some(el)
    }
}

/// Text templates of an element (plate lines, text, ticker); `None` for
/// kinds without text of their own.
pub fn element_texts(el: &Element) -> Option<Vec<&str>> {
    match &el.kind {
        ElementKind::Plate { fields, .. } => Some(fields.iter().map(|f| f.text.as_str()).collect()),
        ElementKind::Text { text, .. } | ElementKind::Ticker { text, .. } => Some(vec![text.as_str()]),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GraphicsConfig {
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub elements: Vec<Element>,
    #[serde(default)]
    pub rules: Vec<GraphicRule>,
    /// Default template variables (e.g. `ticker`); operator values set via
    /// CLI/API override them.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub vars: std::collections::HashMap<String, String>,
}

impl GraphicsConfig {
    pub fn element(&self, id: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.id == id)
    }
    pub(crate) fn layer_exclusive(&self, id: i32) -> bool {
        self.layers.iter().any(|l| l.id == id && l.exclusive)
    }

    /// Layer `id` is not switched off (layers without an entry are on).
    pub fn layer_enabled(&self, id: i32) -> bool {
        self.layers.iter().find(|l| l.id == id).is_none_or(|l| l.enabled)
    }

    pub fn layer(&self, id: i32) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// Name of layer `id` ("Слой N" without an entry or a name).
    pub fn layer_name(&self, id: i32) -> String {
        self.layer(id).map(|l| l.name.trim()).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Слой {id}"))
    }

    /// Ids of every layer: listed ones and those only elements use, top
    /// (highest) first.
    pub fn layer_ids(&self) -> Vec<i32> {
        let mut ids: Vec<i32> = self.layers.iter().map(|l| l.id).chain(self.elements.iter().map(|e| e.layer)).collect();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        ids.dedup();
        ids
    }
}

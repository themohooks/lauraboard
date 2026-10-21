//! Layers, display rules and the graphics configuration file.

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

use super::model::Element;
use crate::Dur;
use crate::rules::Conditions;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: i32,
    #[serde(default)]
    pub name: String,
    /// Only the highest priority element is shown on an exclusive layer.
    #[serde(default)]
    pub exclusive: bool,
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
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GraphicsConfig {
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub elements: Vec<Element>,
    #[serde(default)]
    pub rules: Vec<GraphicRule>,
}

impl GraphicsConfig {
    pub fn element(&self, id: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.id == id)
    }
    pub(crate) fn layer_exclusive(&self, id: i32) -> bool {
        self.layers.iter().any(|l| l.id == id && l.exclusive)
    }
}

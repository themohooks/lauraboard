//! Scenes: library items that are not files but composed on air — a
//! background (colour, still image or a looped video from the library)
//! with music chosen by the scene's own rules. On air a scene is an
//! ordinary item: it is planned, put in playlists and the grid.
//!
//! A scene has graphics of its own (a big clock for "broadcast clock"),
//! kept apart from the channel graphics: editing one never touches the
//! other. The channel graphics (logo, ticker) show over the scene too
//! unless the scene switches them off.
//!
//! * [`music`] — music rules and the music of one airing

use serde::{Deserialize, Serialize};

use crate::WallTime;
use crate::graphics::GraphicsConfig;
use crate::rules::MatchCtx;

mod music;

pub use music::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub background: Background,
    /// Music rules: the first enabled rule whose conditions match the
    /// scene's start time plays; none matching — silence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub music: Vec<MusicRule>,
    /// The scene's own screen graphics (elements and their rules).
    #[serde(default, skip_serializing_if = "is_empty_gfx")]
    pub graphics: GraphicsConfig,
    /// Show the channel graphics over the scene too.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub channel_graphics: bool,
}

impl Default for Scene {
    fn default() -> Self {
        Scene { background: Background::default(), music: vec![], graphics: GraphicsConfig::default(), channel_graphics: true }
    }
}

fn is_empty_gfx(g: &GraphicsConfig) -> bool {
    *g == GraphicsConfig::default()
}

fn is_true(v: &bool) -> bool {
    *v
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Background {
    /// Solid colour `#RRGGBB`.
    Color { color: String },
    /// A library item: a still image, or a video played in a loop (its
    /// own sound is not used).
    Media { media: String },
}

impl Default for Background {
    fn default() -> Self {
        Background::Color { color: "#0B1020".into() }
    }
}

impl Scene {
    /// The music rule for an airing starting `at`.
    pub fn rule_at(&self, at: WallTime) -> Option<&MusicRule> {
        let ctx = MatchCtx { at: Some(at), ..Default::default() };
        self.music.iter().find(|r| r.enabled && r.conditions.matches(&ctx))
    }

    /// Library items the scene needs (background, listed tracks).
    pub fn references(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        if let Background::Media { media } = &self.background {
            v.push(media);
        }
        for r in &self.music {
            v.extend(r.tracks.iter().map(String::as_str));
        }
        v
    }
}

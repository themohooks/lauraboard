//! Media library model.
//!
//! * [`folders`] — virtual folders of the library

pub mod folders;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Dur, Ms};

/// What role a media item plays on air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// Regular content: shows, movies, episodes, clips.
    #[default]
    Program,
    /// Intros, outros, bumpers, idents (заставки).
    Jingle,
    /// Commercials.
    Ad,
    /// Promos / announcements of upcoming programmes.
    Promo,
    /// Short neutral content to fill gaps.
    Filler,
    /// Static image (background, slate).
    Image,
    /// Network stream (http/rtmp/srt/udp …).
    Stream,
}

impl MediaKind {
    pub fn is_interstitial(self) -> bool {
        matches!(self, MediaKind::Jingle | MediaKind::Ad | MediaKind::Promo | MediaKind::Filler)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    #[serde(default)]
    pub codec: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProbeInfo {
    #[serde(default)]
    pub video: Option<VideoInfo>,
    #[serde(default)]
    pub has_audio: bool,
    #[serde(default)]
    pub audio_codec: String,
    #[serde(default)]
    pub format: String,
    /// When probing failed, the error text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Link from a promo to the programme it advertises.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PromoTarget {
    /// Schedule block id (any instance of the block).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    /// Or a media item id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// Or a category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaItem {
    pub id: String,
    /// Absolute path, path relative to the project root, or URL.
    pub path: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Full media duration (from probing or declared for images/streams).
    #[serde(default)]
    pub duration: Dur,
    /// Optional in/out points (trim) — explicitly allowed cuts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_point: Option<Dur>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_point: Option<Dur>,
    /// Gain in dB applied to this item.
    #[serde(default)]
    pub volume_db: f32,
    /// Relative weight for random selection.
    #[serde(default = "one")]
    pub weight: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promo: Option<PromoTarget>,
    /// Presenter / additional info for graphics templates.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub meta: HashMap<String, String>,
    #[serde(default)]
    pub probe: ProbeInfo,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Library folder (`Фильмы/Комедии`; empty = top level).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// File name the media had before it was copied into the channel's
    /// storage under a generated name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
}

fn one() -> f64 {
    1.0
}
fn yes() -> bool {
    true
}

impl MediaItem {
    pub fn new(id: impl Into<String>, path: impl Into<String>, kind: MediaKind, duration: Dur) -> Self {
        let id = id.into();
        MediaItem {
            title: id.clone(),
            id,
            path: path.into(),
            kind,
            category: None,
            tags: vec![],
            duration,
            in_point: None,
            out_point: None,
            volume_db: 0.0,
            weight: 1.0,
            promo: None,
            meta: HashMap::new(),
            probe: ProbeInfo::default(),
            enabled: true,
            folder: String::new(),
            original_name: None,
        }
    }

    pub fn with_category(mut self, c: &str) -> Self {
        self.category = Some(c.to_string());
        self
    }

    /// Effective on-air duration taking in/out points into account.
    pub fn play_duration(&self) -> Ms {
        let start = self.in_point.map(|d| d.ms()).unwrap_or(0);
        let end = self.out_point.map(|d| d.ms()).unwrap_or(self.duration.ms());
        (end - start).max(0)
    }

    pub fn is_url(&self) -> bool {
        is_url(&self.path)
    }

    pub fn has_tag(&self, t: &str) -> bool {
        self.tags.iter().any(|x| x.eq_ignore_ascii_case(t))
    }
}

pub fn is_url(p: &str) -> bool {
    let l = p.to_ascii_lowercase();
    ["http://", "https://", "rtmp://", "rtmps://", "rtsp://", "srt://", "udp://", "tcp://", "rtp://"].iter().any(|s| l.starts_with(s))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Library {
    #[serde(default)]
    pub items: Vec<MediaItem>,
    /// Folders that exist even when empty (folders of items exist anyway).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<String>,
    #[serde(skip)]
    index: HashMap<String, usize>,
}

impl Library {
    pub fn new(items: Vec<MediaItem>) -> Self {
        let mut l = Library { items, folders: vec![], index: HashMap::new() };
        l.reindex();
        l
    }

    pub fn reindex(&mut self) {
        self.index = self.items.iter().enumerate().map(|(i, m)| (m.id.clone(), i)).collect();
    }

    pub fn get(&self, id: &str) -> Option<&MediaItem> {
        self.index.get(id).map(|i| &self.items[*i]).or_else(|| self.items.iter().find(|m| m.id == id))
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut MediaItem> {
        self.items.iter_mut().find(|m| m.id == id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Insert or replace by id.
    pub fn upsert(&mut self, item: MediaItem) {
        if let Some(existing) = self.get_mut(&item.id) {
            *existing = item;
        } else {
            self.items.push(item);
        }
        self.reindex();
    }

    pub fn remove(&mut self, id: &str) -> Option<MediaItem> {
        let pos = self.items.iter().position(|m| m.id == id)?;
        let it = self.items.remove(pos);
        self.reindex();
        Some(it)
    }

    /// Enabled items matching kind and (optionally) category.
    pub fn by_kind_category<'a>(&'a self, kind: Option<MediaKind>, category: Option<&str>) -> impl Iterator<Item = &'a MediaItem> {
        self.items.iter().filter(move |m| {
            m.enabled
                && kind.is_none_or(|k| m.kind == k)
                && category.is_none_or(|c| m.category.as_deref().is_some_and(|mc| mc.eq_ignore_ascii_case(c)))
        })
    }

    /// Generate a unique id from a file name.
    pub fn unique_id(&self, base: &str) -> String {
        let mut slug: String = base.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        while slug.contains("--") {
            slug = slug.replace("--", "-");
        }
        let slug = slug.trim_matches('-').to_string();
        let slug = if slug.is_empty() { "item".to_string() } else { slug };
        if !self.contains(&slug) {
            return slug;
        }
        (2..).map(|n| format!("{slug}-{n}")).find(|s| !self.contains(s)).unwrap()
    }
}

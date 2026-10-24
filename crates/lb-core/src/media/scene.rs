//! Scenes: library items that are not files but composed on air — a
//! background (colour, still image or a looped video from the library)
//! with music chosen by the scene's own rules. On air a scene is an
//! ordinary item: it is planned, put in playlists and the grid.
//!
//! A scene has graphics of its own (a big clock for "broadcast clock"),
//! kept apart from the channel graphics: editing one never touches the
//! other. The channel graphics (logo, ticker) show over the scene too
//! unless the scene switches them off.

use serde::{Deserialize, Serialize};

use crate::graphics::GraphicsConfig;
use crate::rng::Rng;
use crate::rules::{Conditions, MatchCtx};
use crate::{Dur, Ms, WallTime};

use super::{Library, MediaItem, MediaKind};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MusicOrder {
    /// In the listed order, from the first track.
    #[default]
    Sequential,
    /// Shuffled anew for every airing.
    Shuffle,
    /// One random track per airing, repeated if it is short.
    Random,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MusicRule {
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Library items to play (audio, or video whose sound is used).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<String>,
    /// Or / also every enabled item of this library folder (with
    /// sub-folders) …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// … or of this category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub order: MusicOrder,
    /// Gain of the music, dB.
    #[serde(default)]
    pub volume_db: f32,
    /// Fade of the music at the start and the end of the scene.
    #[serde(default = "default_fade")]
    pub fade_in: Dur,
    #[serde(default = "default_fade")]
    pub fade_out: Dur,
    /// Start the first track at a random place (for long mixes).
    #[serde(default, skip_serializing_if = "is_false")]
    pub random_start: bool,
    /// When (weekdays, hours) the rule applies; empty — always.
    #[serde(default)]
    pub conditions: Conditions,
}

fn yes() -> bool {
    true
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn default_fade() -> Dur {
    Dur(1500)
}

impl Default for MusicRule {
    fn default() -> Self {
        MusicRule {
            name: String::new(),
            enabled: true,
            tracks: vec![],
            folder: None,
            category: None,
            order: MusicOrder::Sequential,
            volume_db: 0.0,
            fade_in: default_fade(),
            fade_out: default_fade(),
            random_start: false,
            conditions: Conditions::default(),
        }
    }
}

/// A piece of music in the scene: `media` from `seek_ms` for `play_ms`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub media: String,
    pub seek_ms: Ms,
    pub play_ms: Ms,
}

/// The music of one airing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MusicPlan {
    pub cues: Vec<Cue>,
    pub volume_db: f32,
    pub fade_in_ms: Ms,
    pub fade_out_ms: Ms,
}

/// No more cues than this per airing (each is an FFmpeg input).
const MAX_CUES: usize = 40;

impl MusicRule {
    /// Tracks of the rule that can play (enabled, with a length).
    pub fn pool<'a>(&self, lib: &'a Library) -> Vec<&'a MediaItem> {
        let audible = |m: &&MediaItem| m.enabled && m.scene.is_none() && m.play_duration() > 0 && m.kind != MediaKind::Image && m.kind != MediaKind::Stream;
        let mut v: Vec<&MediaItem> = self.tracks.iter().filter_map(|t| lib.get(t)).filter(audible).collect();
        let folder = self.folder.as_deref().map(super::folders::normalize).filter(|f| !f.is_empty());
        let category = self.category.as_deref().map(str::trim).filter(|c| !c.is_empty());
        if folder.is_some() || category.is_some() {
            for m in lib.items.iter().filter(audible) {
                let in_folder = folder.as_deref().is_some_and(|f| m.folder == f || m.folder.starts_with(&format!("{f}/")));
                let in_cat = category.is_some_and(|c| m.category.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(c)));
                if (in_folder || in_cat) && !v.iter().any(|x| x.id == m.id) {
                    v.push(m);
                }
            }
        }
        v
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

    /// Music for an airing starting `at` (`seed`: the same airing always
    /// gets the same tracks, so a restart or a rewind continues them),
    /// from `offset_ms` into the scene for `duration_ms`.
    pub fn music_plan(&self, lib: &Library, at: WallTime, seed: &str, offset_ms: Ms, duration_ms: Ms) -> MusicPlan {
        let Some(rule) = self.rule_at(at) else { return MusicPlan::default() };
        let pool = rule.pool(lib);
        if pool.is_empty() || duration_ms <= 0 {
            return MusicPlan::default();
        }
        let mut rng = Rng::derive(0, &["scene-music", seed]);
        let order: Vec<&MediaItem> = match rule.order {
            MusicOrder::Sequential => pool.clone(),
            MusicOrder::Shuffle => {
                let mut v = pool.clone();
                for i in (1..v.len()).rev() {
                    v.swap(i, rng.below(i + 1));
                }
                v
            }
            MusicOrder::Random => vec![pool[rng.below(pool.len())]],
        };
        // skip into the first track for long mixes
        let mut first_seek = 0;
        if rule.random_start
            && let Some(m) = order.first()
        {
            first_seek = (rng.next_f64() * (m.play_duration() - 10_000).max(0) as f64) as Ms;
        }
        // walk the looped list up to offset + duration
        let end = offset_ms + duration_ms;
        let mut t: Ms = 0;
        let mut cues = Vec::new();
        let mut i = 0;
        while t < end && cues.len() < MAX_CUES {
            let m = order[i % order.len()];
            let in_point = m.in_point.map(|d| d.ms()).unwrap_or(0);
            let seek0 = if i == 0 { first_seek } else { 0 };
            let len = (m.play_duration() - seek0).max(0);
            if len <= 0 {
                i += 1;
                if i > order.len() * 2 {
                    break;
                }
                continue;
            }
            let (a, b) = (t, t + len);
            if b > offset_ms {
                let from = offset_ms.max(a);
                let to = b.min(end);
                cues.push(Cue { media: m.id.clone(), seek_ms: in_point + seek0 + (from - a), play_ms: to - from });
            }
            t = b;
            i += 1;
        }
        MusicPlan {
            cues,
            volume_db: rule.volume_db,
            // the scene's start / end only (a rewind does not fade in again)
            fade_in_ms: if offset_ms == 0 { rule.fade_in.ms() } else { 0 },
            fade_out_ms: rule.fade_out.ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn lib() -> Library {
        let mut l = Library::default();
        for (id, s) in [("a", 60), ("b", 30), ("c", 90)] {
            let mut m = MediaItem::new(id, format!("{id}.mp3"), MediaKind::Filler, Dur::secs(s));
            m.folder = "Музыка/Спокойная".into();
            l.upsert(m);
        }
        l
    }

    fn at(h: u32) -> WallTime {
        NaiveDate::from_ymd_opt(2026, 10, 24).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    fn scene(order: MusicOrder) -> Scene {
        Scene { music: vec![MusicRule { tracks: vec!["a".into(), "b".into()], order, ..Default::default() }], ..Default::default() }
    }

    #[test]
    fn sequential_covers_and_loops() {
        let p = scene(MusicOrder::Sequential).music_plan(&lib(), at(9), "x", 0, 120_000);
        let ids: Vec<&str> = p.cues.iter().map(|c| c.media.as_str()).collect();
        assert_eq!(ids, ["a", "b", "a"]);
        assert_eq!(p.cues.iter().map(|c| c.play_ms).sum::<Ms>(), 120_000);
        assert_eq!(p.cues[2].play_ms, 30_000);
    }

    #[test]
    fn offset_starts_inside_a_track() {
        let p = scene(MusicOrder::Sequential).music_plan(&lib(), at(9), "x", 70_000, 10_000);
        assert_eq!(p.cues, vec![Cue { media: "b".into(), seek_ms: 10_000, play_ms: 10_000 }]);
        assert_eq!(p.fade_in_ms, 0);
    }

    #[test]
    fn shuffle_is_stable_for_one_airing() {
        let s = scene(MusicOrder::Shuffle);
        assert_eq!(s.music_plan(&lib(), at(9), "uid-1", 0, 200_000), s.music_plan(&lib(), at(9), "uid-1", 0, 200_000));
    }

    #[test]
    fn rules_by_hour_and_folder_pool() {
        let night = MusicRule {
            folder: Some("Музыка".into()),
            conditions: Conditions { from: chrono::NaiveTime::from_hms_opt(22, 0, 0), to: chrono::NaiveTime::from_hms_opt(6, 0, 0), ..Default::default() },
            ..Default::default()
        };
        let day = MusicRule { tracks: vec!["b".into()], ..Default::default() };
        let s = Scene { music: vec![night, day], ..Default::default() };
        assert_eq!(s.music_plan(&lib(), at(23), "x", 0, 10_000).cues[0].media, "a");
        assert_eq!(s.music_plan(&lib(), at(12), "x", 0, 10_000).cues[0].media, "b");
        assert_eq!(s.rule_at(at(23)).unwrap().pool(&lib()).len(), 3);
    }

    #[test]
    fn no_rule_no_music() {
        assert!(Scene::default().music_plan(&lib(), at(9), "x", 0, 10_000).cues.is_empty());
    }
}

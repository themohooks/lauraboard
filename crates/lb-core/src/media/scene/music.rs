//! Music of a scene: rules (which tracks, in what order, how many times,
//! with what pauses) and the music of one airing as a list of cues.

use serde::{Deserialize, Serialize};

use crate::rng::Rng;
use crate::rules::Conditions;
use crate::{Dur, Ms, WallTime};

use super::super::{Library, MediaItem, MediaKind};
use super::{Scene, yes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MusicOrder {
    /// In the listed order, from the first track.
    #[default]
    Sequential,
    /// Shuffled anew for every airing.
    Shuffle,
    /// One random track per airing.
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
    /// How many times the list (the one track for `Random`) plays per
    /// airing; 0 — over and over until the scene ends. Once it has played
    /// the scene goes on in silence.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub repeat: u32,
    /// Silence between tracks.
    #[serde(default, skip_serializing_if = "no_time")]
    pub gap: Dur,
    /// Silence at the start of the scene before the music.
    #[serde(default, skip_serializing_if = "no_time")]
    pub delay: Dur,
    /// Gain of the music, dB.
    #[serde(default)]
    pub volume_db: f32,
    /// Fade of the music at its start and its end (the end of the scene,
    /// or of the music when it plays a set number of times).
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

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn no_time(d: &Dur) -> bool {
    d.ms() == 0
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
            repeat: 0,
            gap: Dur(0),
            delay: Dur(0),
            volume_db: 0.0,
            fade_in: default_fade(),
            fade_out: default_fade(),
            random_start: false,
            conditions: Conditions::default(),
        }
    }
}

/// A piece of the scene's sound: `media` from `seek_ms` for `play_ms`;
/// an empty `media` is silence (a pause, the delay before the music).
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub media: String,
    pub seek_ms: Ms,
    pub play_ms: Ms,
}

impl Cue {
    pub fn is_silence(&self) -> bool {
        self.media.is_empty()
    }
}

/// The music of one airing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MusicPlan {
    pub cues: Vec<Cue>,
    pub volume_db: f32,
    pub fade_in_ms: Ms,
    pub fade_out_ms: Ms,
    /// The music ends this long into the airing (it plays a set number of
    /// times and is shorter than the scene); `None` — it lasts to the end.
    pub end_ms: Option<Ms>,
}

/// No more cues than this per airing (each is an FFmpeg input).
const MAX_CUES: usize = 40;

impl MusicRule {
    /// Tracks of the rule that can play (enabled, with a length).
    pub fn pool<'a>(&self, lib: &'a Library) -> Vec<&'a MediaItem> {
        let audible = |m: &&MediaItem| m.enabled && m.scene.is_none() && m.play_duration() > 0 && m.kind != MediaKind::Image && m.kind != MediaKind::Stream;
        let mut v: Vec<&MediaItem> = self.tracks.iter().filter_map(|t| lib.get(t)).filter(audible).collect();
        let folder = self.folder.as_deref().map(super::super::folders::normalize).filter(|f| !f.is_empty());
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

    /// Length of the music with a set number of plays (delay, tracks and
    /// pauses), when it is known in advance — not for one random track
    /// or a random start. `None` also for endless music.
    pub fn length(&self, lib: &Library) -> Option<Ms> {
        if self.repeat == 0 || self.order == MusicOrder::Random || self.random_start {
            return None;
        }
        let pool = self.pool(lib);
        if pool.is_empty() {
            return None;
        }
        let one: Ms = pool.iter().map(|m| m.play_duration()).sum();
        let plays = (pool.len() as i64) * self.repeat as i64;
        Some(self.delay.ms() + one * self.repeat as i64 + self.gap.ms() * (plays - 1))
    }
}

/// One segment of the sound before cutting to the airing: track (or
/// silence), where it starts in the file, its length.
struct Segment<'a> {
    media: Option<&'a MediaItem>,
    seek: Ms,
    len: Ms,
}

impl Scene {
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
        let end = offset_ms + duration_ms;
        let segments = segments(rule, &order, first_seek, end);
        // cut the segments to the airing
        let mut t: Ms = 0;
        let mut cues = Vec::new();
        for s in &segments {
            let (a, b) = (t, t + s.len);
            t = b;
            if b <= offset_ms || cues.len() >= MAX_CUES {
                continue;
            }
            let from = offset_ms.max(a);
            let to = b.min(end);
            if to <= from {
                continue;
            }
            let (media, seek) = match s.media {
                Some(m) => (m.id.clone(), s.seek + (from - a)),
                None => (String::new(), 0),
            };
            cues.push(Cue { media, seek_ms: seek, play_ms: to - from });
        }
        // nothing but silence (the music is over before this point)
        if cues.iter().all(Cue::is_silence) {
            return MusicPlan::default();
        }
        // silence after the last track is the scene's own
        while cues.last().is_some_and(Cue::is_silence) {
            cues.pop();
        }
        let music_end = t;
        let delay = rule.delay.ms();
        MusicPlan {
            cues,
            volume_db: rule.volume_db,
            // fades at the music's own start / end (a rewind past the start
            // does not fade in again)
            fade_in_ms: if offset_ms <= delay { rule.fade_in.ms() } else { 0 },
            fade_out_ms: rule.fade_out.ms(),
            end_ms: (music_end < end).then_some(music_end - offset_ms),
        }
    }
}

/// The sound of the scene from its start up to `end` (or the end of a
/// set number of plays): the delay, the tracks and the pauses between them.
fn segments<'a>(rule: &MusicRule, order: &[&'a MediaItem], first_seek: Ms, end: Ms) -> Vec<Segment<'a>> {
    let mut v: Vec<Segment<'a>> = Vec::new();
    let mut t: Ms = 0;
    if rule.delay.ms() > 0 {
        v.push(Segment { media: None, seek: 0, len: rule.delay.ms() });
        t += rule.delay.ms();
    }
    let total = if rule.repeat == 0 { usize::MAX } else { order.len() * rule.repeat as usize };
    let mut played = 0;
    let mut i = 0;
    // a cap on the walk: every track may be too short to count
    while t < end && played < total && i < order.len() * 2 + MAX_CUES * 4 {
        let m = order[i % order.len()];
        i += 1;
        let in_point = m.in_point.map(|d| d.ms()).unwrap_or(0);
        let seek0 = if played == 0 { first_seek } else { 0 };
        let len = (m.play_duration() - seek0).max(0);
        if len <= 0 {
            continue;
        }
        if played > 0 && rule.gap.ms() > 0 {
            v.push(Segment { media: None, seek: 0, len: rule.gap.ms() });
            t += rule.gap.ms();
        }
        v.push(Segment { media: Some(m), seek: in_point + seek0, len });
        t += len;
        played += 1;
    }
    v
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

    fn rule(order: MusicOrder) -> MusicRule {
        MusicRule { tracks: vec!["a".into(), "b".into()], order, ..Default::default() }
    }

    fn scene_of(r: MusicRule) -> Scene {
        Scene { music: vec![r], ..Default::default() }
    }

    fn scene(order: MusicOrder) -> Scene {
        scene_of(rule(order))
    }

    fn ids(p: &MusicPlan) -> Vec<&str> {
        p.cues.iter().map(|c| if c.is_silence() { "-" } else { c.media.as_str() }).collect()
    }

    #[test]
    fn sequential_covers_and_loops() {
        let p = scene(MusicOrder::Sequential).music_plan(&lib(), at(9), "x", 0, 120_000);
        assert_eq!(ids(&p), ["a", "b", "a"]);
        assert_eq!(p.cues.iter().map(|c| c.play_ms).sum::<Ms>(), 120_000);
        assert_eq!(p.cues[2].play_ms, 30_000);
        assert_eq!(p.end_ms, None);
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

    #[test]
    fn plays_a_set_number_of_times_then_silence() {
        let s = scene_of(MusicRule { repeat: 1, ..rule(MusicOrder::Sequential) });
        let p = s.music_plan(&lib(), at(9), "x", 0, 300_000);
        assert_eq!(ids(&p), ["a", "b"]);
        assert_eq!(p.end_ms, Some(90_000));
        assert_eq!(s.music[0].length(&lib()), Some(90_000));
        // the music is over before a rewind point: silence
        assert!(s.music_plan(&lib(), at(9), "x", 120_000, 60_000).cues.is_empty());
        let twice = scene_of(MusicRule { repeat: 2, order: MusicOrder::Random, ..rule(MusicOrder::Sequential) });
        let p = twice.music_plan(&lib(), at(9), "x", 0, 300_000);
        assert_eq!(p.cues.len(), 2);
        assert_eq!(p.cues[0].media, p.cues[1].media);
    }

    #[test]
    fn delay_and_pauses() {
        let r = MusicRule { repeat: 1, delay: Dur::secs(5), gap: Dur::secs(2), ..rule(MusicOrder::Sequential) };
        assert_eq!(r.length(&lib()), Some(5_000 + 60_000 + 2_000 + 30_000));
        let s = scene_of(r);
        let p = s.music_plan(&lib(), at(9), "x", 0, 300_000);
        assert_eq!(ids(&p), ["-", "a", "-", "b"]);
        assert_eq!(p.cues[2].play_ms, 2_000);
        assert_eq!(p.fade_in_ms, 1500);
        assert_eq!(p.end_ms, Some(97_000));
        // inside the pause
        let p = s.music_plan(&lib(), at(9), "x", 66_000, 10_000);
        assert_eq!(ids(&p), ["-", "b"]);
        assert_eq!(p.cues[0].play_ms, 1_000);
        assert_eq!(p.fade_in_ms, 0);
    }
}

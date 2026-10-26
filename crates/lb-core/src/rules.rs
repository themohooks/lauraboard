//! Interstitial insertion rules: jingles (заставки), ad breaks, promos.

use chrono::{Datelike, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

use crate::media::MediaKind;
use crate::{Dur, WallTime};

/// Where a rule fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// After every N main items within a block.
    EveryNItems {
        n: usize,
    },
    /// After a main item once at least this much time passed since the rule
    /// last fired (or since the block start).
    EveryMinutes {
        minutes: f64,
    },
    /// At the start of a block.
    BlockStart,
    /// At the end of a block (before gap filling).
    BlockEnd,
    /// Between two blocks (after BlockEnd of the previous block).
    BetweenBlocks,
    /// After a main item of the category.
    AfterCategory {
        category: String,
    },
    /// Before a main item of the category.
    BeforeCategory {
        category: String,
    },
    /// After any of these library items (as main content).
    AfterMedia {
        items: Vec<String>,
    },
    /// After the last entry of a playlist (each time a pass of it ends).
    AfterPlaylist {
        playlist: String,
    },
    /// After the first main item ending at or past each of these daily
    /// times (once per time).
    AtTimes {
        times: Vec<NaiveTime>,
    },
    /// At this minute and second of every hour.
    Hourly {
        minute: u32,
        #[serde(default)]
        second: u32,
    },
    BroadcastStart,
    BroadcastEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeConflict {
    #[default]
    Finish,
    Truncate,
}

impl Trigger {
    /// First scheduled occurrence in the inclusive interval.
    pub fn occurrence(&self, from: WallTime, until: WallTime) -> Option<WallTime> {
        let mut day = from.date();
        let mut first = None;
        while day <= until.date() {
            let times = match self {
                Self::AtTimes { times } => times.clone(),
                Self::Hourly { minute, second } => (0..24).filter_map(|h| NaiveTime::from_hms_opt(h, *minute, *second)).collect(),
                _ => return None,
            };
            for time in times {
                let at = day.and_time(time);
                if at >= from && at <= until {
                    first = Some(first.map_or(at, |old: WallTime| old.min(at)));
                }
            }
            day = day.succ_opt()?;
        }
        first
    }
}

/// What gets inserted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InsertAction {
    /// One jingle chosen from a category of `jingle` media (or explicit ids).
    Jingle {
        #[serde(default)]
        category: Option<String>,
        #[serde(default)]
        items: Vec<String>,
    },
    /// An ad break defined in [`AdBreak`].
    AdBreak { ad_break: String },
    /// N promos selected automatically from currently relevant promos.
    Promos {
        #[serde(default = "one_usize")]
        count: usize,
        #[serde(default)]
        category: Option<String>,
    },
    /// Arbitrary media (one picked).
    Media { items: Vec<String> },
    /// A scene (one picked from `items`; empty — any enabled scene) shown
    /// for `duration` (`None` — the scene's own length).
    Scene {
        #[serde(default)]
        items: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration: Option<Dur>,
    },
}

fn one_usize() -> usize {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    /// Rotate through candidates in order.
    #[default]
    Sequential,
    /// Uniform / weighted random (uses media `weight`).
    Random,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Conditions {
    /// Main item category must be one of these (empty = any).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub playlists: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_media: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<MediaKind>,
    /// ISO weekdays 1 (Mon) … 7 (Sun); empty = every day.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weekdays: Vec<u32>,
    /// Daily window `from`..`to` (wraps over midnight when to < from).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<NaiveTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<NaiveTime>,
}

/// Facts about the context a rule is evaluated in.
#[derive(Debug, Clone, Default)]
pub struct MatchCtx<'a> {
    pub at: Option<WallTime>,
    pub category: Option<&'a str>,
    pub playlist: Option<&'a str>,
    pub block: Option<&'a str>,
    pub media: Option<&'a str>,
    pub kind: Option<MediaKind>,
}

fn ci_contains(list: &[String], v: Option<&str>) -> bool {
    v.is_some_and(|v| list.iter().any(|x| x.eq_ignore_ascii_case(v)))
}

impl Conditions {
    pub fn matches(&self, c: &MatchCtx) -> bool {
        if !self.categories.is_empty() && !ci_contains(&self.categories, c.category) {
            return false;
        }
        if ci_contains(&self.exclude_categories, c.category) {
            return false;
        }
        if !self.playlists.is_empty() && !ci_contains(&self.playlists, c.playlist) {
            return false;
        }
        if !self.blocks.is_empty() && !ci_contains(&self.blocks, c.block) {
            return false;
        }
        if !self.media.is_empty() && !ci_contains(&self.media, c.media) {
            return false;
        }
        if ci_contains(&self.exclude_media, c.media) {
            return false;
        }
        if !self.kinds.is_empty() && !c.kind.is_some_and(|k| self.kinds.contains(&k)) {
            return false;
        }
        if let Some(at) = c.at {
            if !self.weekdays.is_empty() && !self.weekdays.contains(&at.weekday().number_from_monday()) {
                return false;
            }
            if !time_in_window(at.time(), self.from, self.to) {
                return false;
            }
        }
        true
    }
}

/// Whether `t` lies inside `[from, to)`; open ends are unbounded; wraps
/// across midnight when `to <= from`.
pub fn time_in_window(t: NaiveTime, from: Option<NaiveTime>, to: Option<NaiveTime>) -> bool {
    let t = t.num_seconds_from_midnight();
    match (from.map(|x| x.num_seconds_from_midnight()), to.map(|x| x.num_seconds_from_midnight())) {
        (None, None) => true,
        (Some(f), None) => t >= f,
        (None, Some(e)) => t < e,
        (Some(f), Some(e)) if f < e => t >= f && t < e,
        (Some(f), Some(e)) => t >= f || t < e,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsertionRule {
    /// Whether a timed insertion may interrupt main content.
    #[serde(default)]
    pub time_conflict: TimeConflict,
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub trigger: Trigger,
    pub action: InsertAction,
    #[serde(default)]
    pub conditions: Conditions,
    #[serde(default)]
    pub select: SelectMode,
    /// Chance the rule fires when triggered (0..1).
    #[serde(default = "one_f")]
    pub probability: f64,
    /// Never pick the same media twice in a row for this rule.
    #[serde(default = "yes")]
    pub no_repeat: bool,
    /// Higher fires first; within the same `group` only the highest fires.
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Optional items may be dropped to meet a fixed start time.
    #[serde(default = "yes")]
    pub optional: bool,
    /// Skip when the operator switches manually.
    #[serde(default = "yes")]
    pub skip_on_manual: bool,
}

fn yes() -> bool {
    true
}
fn one_f() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AdOrder {
    /// Order as listed.
    #[default]
    Listed,
    Random,
    /// Ads with the fewest airings today first.
    LeastPlayed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdBreak {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Explicit ad media ids; if empty, all `ad` media of `category`.
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub order: AdOrder,
    /// Fixed total length; filled up with `fill_category` fillers / slate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Dur>,
    /// Maximum number of spots in the break.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_spots: Option<usize>,
    /// Bumpers (jingle media ids) before / after the break.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outro: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdPolicy {
    /// Max airings of one spot per day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_day: Option<u32>,
    /// Minimum time between two airings of the same spot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_interval: Option<Dur>,
}

impl Default for AdPolicy {
    fn default() -> Self {
        AdPolicy { max_per_day: None, min_interval: Some(Dur::mins(30)) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromoPolicy {
    /// Only promote programmes starting within this window.
    #[serde(default = "default_lookahead")]
    pub lookahead: Dur,
    /// Do not repeat the same promo within this interval.
    #[serde(default = "default_promo_interval")]
    pub min_interval: Dur,
}

fn default_lookahead() -> Dur {
    Dur::mins(24 * 60)
}
fn default_promo_interval() -> Dur {
    Dur::mins(60)
}

impl Default for PromoPolicy {
    fn default() -> Self {
        PromoPolicy { lookahead: default_lookahead(), min_interval: default_promo_interval() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillerPolicy {
    /// Categories of filler media used for gaps (empty = all filler media).
    #[serde(default)]
    pub categories: Vec<String>,
    /// Allow promos in gaps.
    #[serde(default = "yes")]
    pub use_promos: bool,
    /// Gaps shorter than this are covered by the slate directly.
    #[serde(default = "default_min_gap")]
    pub min_fill: Dur,
    /// Do not reuse the same filler within this interval.
    #[serde(default = "default_filler_interval")]
    pub min_interval: Dur,
    /// Scenes that cover what fillers leave of a gap (instead of the
    /// slate), stretched to it; one is picked in turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<String>,
    /// A scene takes the whole gap (no fillers / promos before it).
    #[serde(default, skip_serializing_if = "is_false")]
    pub scenes_first: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn default_min_gap() -> Dur {
    Dur::secs(3)
}
fn default_filler_interval() -> Dur {
    Dur::mins(30)
}

impl Default for FillerPolicy {
    fn default() -> Self {
        FillerPolicy {
            categories: vec![],
            use_promos: true,
            min_fill: default_min_gap(),
            min_interval: default_filler_interval(),
            scenes: vec![],
            scenes_first: false,
        }
    }
}

/// Transition between items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transitions {
    /// Fade in/out (to/from black) length applied to items; 0 = cut.
    #[serde(default)]
    pub fade: Dur,
    /// Kinds that get fades (empty = all).
    #[serde(default)]
    pub kinds: Vec<MediaKind>,
}

impl Default for Transitions {
    fn default() -> Self {
        Transitions { fade: Dur(0), kinds: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RuleSet {
    #[serde(default)]
    pub rules: Vec<InsertionRule>,
    #[serde(default)]
    pub ad_breaks: Vec<AdBreak>,
    #[serde(default)]
    pub ads: AdPolicy,
    #[serde(default)]
    pub promos: PromoPolicy,
    #[serde(default)]
    pub filler: FillerPolicy,
    #[serde(default)]
    pub transitions: Transitions,
}

impl RuleSet {
    pub fn ad_break(&self, id: &str) -> Option<&AdBreak> {
        self.ad_breaks.iter().find(|b| b.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn windows() {
        assert!(time_in_window(t(10), Some(t(9)), Some(t(12))));
        assert!(!time_in_window(t(13), Some(t(9)), Some(t(12))));
        assert!(time_in_window(t(23), Some(t(22)), Some(t(2))));
        assert!(time_in_window(t(1), Some(t(22)), Some(t(2))));
        assert!(!time_in_window(t(3), Some(t(22)), Some(t(2))));
    }

    #[test]
    fn conditions() {
        let c = Conditions { categories: vec!["news".into()], exclude_media: vec!["x".into()], weekdays: vec![3], ..Default::default() };
        let wed = chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(10, 0, 0).unwrap();
        let ctx = MatchCtx { at: Some(wed), category: Some("News"), media: Some("y"), ..Default::default() };
        assert!(c.matches(&ctx));
        assert!(!c.matches(&MatchCtx { media: Some("x"), ..ctx.clone() }));
        assert!(!c.matches(&MatchCtx { category: Some("films"), ..ctx.clone() }));
        let thu = wed + chrono::Duration::days(1);
        assert!(!c.matches(&MatchCtx { at: Some(thu), ..ctx }));
    }
}

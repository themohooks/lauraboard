//! Cross-reference checks between library, playlists, schedules, rules and
//! graphics.

use std::collections::HashSet;

use super::Report;
use crate::graphics::ElementKind;
use crate::media::MediaKind;
use crate::project::ChannelData;
use crate::rules::{InsertAction, Trigger};
use crate::schedule::BlockSource;

pub(super) fn check(ch: &ChannelData, r: &mut Report) {
    library(ch, r);
    playlists(ch, r);
    schedules(ch, r);
    rules(ch, r);
    graphics(ch, r);
}

fn library(ch: &ChannelData, r: &mut Report) {
    let mut seen = HashSet::new();
    for m in &ch.library.items {
        if !seen.insert(m.id.as_str()) {
            r.error("library", format!("duplicate media id '{}'", m.id));
        }
        if m.duration.ms() <= 0 && !matches!(m.kind, MediaKind::Image | MediaKind::Stream) {
            r.warn("library", format!("media '{}' has no duration (not probed?)", m.id));
        }
        if m.play_duration() <= 0 && m.duration.ms() > 0 {
            r.error("library", format!("media '{}' in/out points leave nothing to play", m.id));
        }
    }
}

fn playlists(ch: &ChannelData, r: &mut Report) {
    for p in &ch.playlists {
        let loc = format!("playlists/{}", p.id);
        if p.entries.is_empty() {
            r.warn(&loc, "playlist is empty");
        }
        for e in &p.entries {
            if !ch.library.contains(&e.media) {
                r.error(&loc, format!("unknown media '{}'", e.media));
            }
        }
    }
}

fn schedules(ch: &ChannelData, r: &mut Report) {
    let mut ids = HashSet::new();
    for s in &ch.schedules.schedules {
        if !ids.insert(s.id.as_str()) {
            r.error("schedule", format!("duplicate schedule id '{}'", s.id));
        }
        let wk = &s.week;
        for t in [&wk.mon, &wk.tue, &wk.wed, &wk.thu, &wk.fri, &wk.sat, &wk.sun, &wk.default].into_iter().flatten() {
            if s.template(t).is_none() {
                r.error(format!("schedule/{}", s.id), format!("week map references unknown template '{t}'"));
            }
        }
        for ex in &s.exceptions {
            if let Some(t) = &ex.template
                && s.template(t).is_none()
            {
                r.error(format!("schedule/{}", s.id), format!("exception {} references unknown template '{t}'", ex.date));
            }
        }
        for t in &s.templates {
            let loc = format!("schedule/{}/{}", s.id, t.id);
            let mut last = None;
            for b in &t.blocks {
                if let Some(st) = b.start {
                    if last.is_some_and(|l| st <= l) {
                        r.error(&loc, format!("block '{}' at {st} overlaps / is out of order", b.id));
                    }
                    last = Some(st);
                }
                block_source(ch, &loc, &b.id, &b.source, r);
                for bm in [&b.intro, &b.outro].into_iter().flatten() {
                    if !ch.library.contains(bm) {
                        r.error(&loc, format!("block '{}' intro/outro '{bm}' not in library", b.id));
                    }
                }
            }
            if t.blocks.first().is_some_and(|b| b.start.is_none()) {
                r.warn(&loc, "first block has no fixed start; it follows the previous day");
            }
        }
    }
    if ch.schedules.schedules.is_empty() && matches!(ch.config.planning.mode, crate::planner::PlanMode::Grid) {
        r.warn("schedule", "grid mode without schedules — only the slate will air");
    }
}

fn block_source(ch: &ChannelData, loc: &str, block: &str, src: &BlockSource, r: &mut Report) {
    match src {
        BlockSource::Playlist { playlist, .. } if ch.playlist(playlist).is_none() => {
            r.error(loc, format!("block '{block}' uses unknown playlist '{playlist}'"))
        }
        BlockSource::Media { items } => {
            for i in items.iter().filter(|i| !ch.library.contains(i)) {
                r.error(loc, format!("block '{block}' uses unknown media '{i}'"));
            }
        }
        BlockSource::Category { category, kind } => {
            if ch.library.by_kind_category(Some(kind.unwrap_or(MediaKind::Program)), Some(category)).next().is_none() {
                r.warn(loc, format!("block '{block}': no media in category '{category}'"));
            }
        }
        BlockSource::AdBreak { ad_break } if ch.rules.ad_break(ad_break).is_none() => {
            r.error(loc, format!("block '{block}' uses unknown ad break '{ad_break}'"))
        }
        _ => {}
    }
}

fn rules(ch: &ChannelData, r: &mut Report) {
    for rule in &ch.rules.rules {
        let loc = format!("rules/{}", rule.id);
        if !(0.0..=1.0).contains(&rule.probability) {
            r.error(&loc, "probability must be between 0 and 1");
        }
        match &rule.action {
            InsertAction::AdBreak { ad_break } if ch.rules.ad_break(ad_break).is_none() => {
                r.error(&loc, format!("unknown ad break '{ad_break}'"))
            }
            InsertAction::Jingle { items, .. } | InsertAction::Media { items } => {
                for i in items.iter().filter(|i| !ch.library.contains(i)) {
                    r.error(&loc, format!("unknown media '{i}'"));
                }
            }
            InsertAction::Scene { items, duration } => {
                for i in items {
                    match ch.library.get(i) {
                        None => r.error(&loc, format!("unknown media '{i}'")),
                        Some(m) if !m.is_scene() => r.error(&loc, format!("'{i}' is not a scene")),
                        _ => {}
                    }
                }
                if duration.is_some_and(|d| d.ms() <= 0) {
                    r.error(&loc, "scene duration must be positive");
                }
            }
            _ => {}
        }
        match &rule.trigger {
            Trigger::AfterMedia { items } => {
                for i in items.iter().filter(|i| !ch.library.contains(i)) {
                    r.warn(&loc, format!("trigger: unknown media '{i}'"));
                }
            }
            Trigger::AfterPlaylist { playlist } if !ch.playlists.iter().any(|p| &p.id == playlist) => {
                r.warn(&loc, format!("trigger: unknown playlist '{playlist}'"))
            }
            Trigger::AtTimes { times } if times.is_empty() => r.warn(&loc, "trigger: no times"),
            _ => {}
        }
    }
    for i in &ch.rules.filler.scenes {
        match ch.library.get(i) {
            None => r.error("rules/filler", format!("unknown media '{i}'")),
            Some(m) if !m.is_scene() => r.error("rules/filler", format!("'{i}' is not a scene")),
            _ => {}
        }
    }
    for b in &ch.rules.ad_breaks {
        for i in b.items.iter().filter(|i| !ch.library.contains(i)) {
            r.error(format!("ad_breaks/{}", b.id), format!("unknown media '{i}'"));
        }
    }
}

fn graphics(ch: &ChannelData, r: &mut Report) {
    graphics_doc(&ch.graphics, "graphics", r);
    for m in ch.library.items.iter() {
        if let Some(s) = &m.scene {
            graphics_doc(&s.graphics, &format!("library/{}/graphics", m.id), r);
        }
    }
}

/// References inside one graphics document (the channel's or a scene's).
fn graphics_doc(g: &crate::graphics::GraphicsConfig, loc: &str, r: &mut Report) {
    for rule in &g.rules {
        if g.element(&rule.element).is_none() {
            r.error(format!("{loc}/rules/{}", rule.id), format!("unknown element '{}'", rule.element));
        }
    }
    for e in &g.elements {
        if let ElementKind::Clock { format, .. } = &e.kind
            && !valid_time_format(format)
        {
            r.warn(format!("{loc}/{}", e.id), format!("invalid clock format '{format}'"));
        }
        if let ElementKind::Group { children } = &e.kind {
            for c in children.iter().filter(|c| g.element(c).is_none()) {
                r.error(format!("{loc}/{}", e.id), format!("unknown child element '{c}'"));
            }
        }
    }
}

/// `strftime` pattern chrono can format (`%Q` or a trailing `%` cannot).
fn valid_time_format(f: &str) -> bool {
    use chrono::format::{Item, StrftimeItems};
    !StrftimeItems::new(f).any(|i| matches!(i, Item::Error))
}

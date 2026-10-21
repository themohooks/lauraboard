//! Block content sources (playlist / media list / category / ad break) and
//! selection helpers.

use crate::history::PlayHistory;
use crate::media::{MediaItem, MediaKind};
use crate::playlist::Cursor;
use crate::rundown::WarningKind;
use crate::schedule::{Block, BlockSource};
use crate::{Ms, WallTime, diff_ms};

use super::context::{MatchCtxOwned, Planner, Src};

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    /// `items_done`: main items of this block instance already aired (when
    /// continuing a block).
    pub(super) fn src_for(&self, b: &'a Block, items_done: usize) -> Src<'a> {
        match &b.source {
            BlockSource::Playlist { playlist, restart } => match self.playlist(playlist) {
                Some(pl) => {
                    let cursor = if *restart && items_done == 0 {
                        Cursor::default()
                    } else {
                        self.st.cursors.get(playlist).copied().unwrap_or_default()
                    };
                    Src::Playlist { pl, cursor }
                }
                None => Src::Empty,
            },
            BlockSource::Media { items } => Src::Media { items, pos: items_done.min(items.len()) },
            BlockSource::Category { category, kind } => Src::Category { category, kind: kind.unwrap_or(MediaKind::Program) },
            BlockSource::AdBreak { ad_break } => match self.inp.rules.ad_break(ad_break) {
                Some(brk) => Src::AdBreak { brk },
                None => Src::Empty,
            },
        }
    }

    /// Next main candidate from the source *without* committing. Returns the
    /// media and the source state after taking it.
    pub(super) fn peek_src(
        &mut self,
        src: &Src<'a>,
        block: &Block,
        t: WallTime,
        avoid: Option<&str>,
        max_ms: Option<Ms>,
    ) -> Option<(&'a MediaItem, Src<'a>)> {
        match src {
            Src::Playlist { pl, cursor } => {
                let mut c = *cursor;
                let n = pl.entries.len().max(1);
                for _ in 0..n * 2 {
                    let idx = c.next(pl, self.inp.seed)?;
                    let e = &pl.entries[idx];
                    let Some(m) = self.media(&e.media) else {
                        self.warn(
                            WarningKind::MissingMedia,
                            Some(t),
                            format!("playlist '{}' references unknown media '{}'", pl.id, e.media),
                        );
                        continue;
                    };
                    if !m.enabled || m.play_duration() <= 0 || self.recently_played(m, block, t) {
                        continue;
                    }
                    return Some((m, Src::Playlist { pl, cursor: c }));
                }
                None
            }
            Src::Media { items, pos } => {
                let mut p = *pos;
                while p < items.len() {
                    let id = &items[p];
                    p += 1;
                    match self.media(id) {
                        Some(m) if m.enabled && m.play_duration() > 0 => return Some((m, Src::Media { items, pos: p })),
                        _ => self.warn(
                            WarningKind::MissingMedia,
                            Some(t),
                            format!("block '{}' references unavailable media '{id}'", block.id),
                        ),
                    }
                }
                None
            }
            Src::Category { category, kind } => {
                let cands: Vec<&'a MediaItem> = self
                    .inp
                    .library
                    .by_kind_category(Some(*kind), Some(category))
                    .filter(|m| m.play_duration() > 0)
                    .filter(|m| max_ms.is_none_or(|mx| m.play_duration() <= mx))
                    .filter(|m| !self.recently_played(m, block, t))
                    .collect();
                let pick = self.pick_weighted(&cands, avoid)?;
                Some((pick, Src::Category { category, kind: *kind }))
            }
            Src::AdBreak { .. } | Src::Empty => None,
        }
    }

    /// Aired within the block's `exclude_recent` window.
    pub(super) fn recently_played(&self, m: &MediaItem, block: &Block, t: WallTime) -> bool {
        block.exclude_recent.is_some_and(|w| self.hist.last_played(&m.id).is_some_and(|lp| diff_ms(t, lp) < w.ms()))
    }

    /// Weighted random pick, avoiding `avoid` when there is a choice.
    pub(super) fn pick_weighted(&mut self, cands: &[&'a MediaItem], avoid: Option<&str>) -> Option<&'a MediaItem> {
        if cands.is_empty() {
            return None;
        }
        let filtered: Vec<&'a MediaItem> =
            if cands.len() > 1 { cands.iter().copied().filter(|m| Some(m.id.as_str()) != avoid).collect() } else { cands.to_vec() };
        let w: Vec<f64> = filtered.iter().map(|m| m.weight.max(0.0)).collect();
        let i = self.rng.weighted(&w).unwrap_or_else(|| self.rng.below(filtered.len()));
        filtered.get(i).copied()
    }

    /// Category used for rule matching: block > media > playlist.
    pub(super) fn effective_category(&self, m: &MediaItem, block: &Block, src: &Src) -> Option<String> {
        block
            .category
            .clone()
            .or_else(|| m.category.clone())
            .or_else(|| if let Src::Playlist { pl, .. } = src { pl.category.clone() } else { None })
    }

    pub(super) fn block_ctx(&self, b: &'a Block, at: WallTime) -> MatchCtxOwned {
        MatchCtxOwned { at, category: b.category.clone(), playlist: block_playlist(b), block: Some(b.id.clone()), media: None, kind: None }
    }
}

pub(super) fn block_playlist(b: &Block) -> Option<String> {
    match &b.source {
        BlockSource::Playlist { playlist, .. } => Some(playlist.clone()),
        _ => None,
    }
}

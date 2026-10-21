//! Planner working state shared by all planning stages.

use crate::WallTime;
use crate::history::Overlay;
use crate::media::{MediaItem, MediaKind};
use crate::playlist::{Cursor, Playlist};
use crate::rng::Rng;
use crate::rules::{AdBreak, MatchCtx};
use crate::rundown::{PlanWarning, RundownItem, WarningKind};
use crate::schedule::Block;

use super::{PlanInput, PlanState};

/// The planner: one instance per planning pass.
pub(super) struct Planner<'a, 'h> {
    pub(super) inp: &'a PlanInput<'a>,
    /// Working copy of the persisted state (cursors, rotations).
    pub(super) st: PlanState,
    /// Real history plus what was planned so far in this pass.
    pub(super) hist: Overlay<'h>,
    pub(super) rng: Rng,
    pub(super) out: Vec<RundownItem>,
    pub(super) warnings: Vec<PlanWarning>,
    pub(super) seq: u64,
    pub(super) uid_base: String,
}

/// A block occurrence being planned.
#[derive(Clone)]
pub(super) struct Inst<'a> {
    /// `date:block_id`
    pub(super) key: String,
    pub(super) block: &'a Block,
    pub(super) fixed: Option<WallTime>,
}

/// Source iterator state for the block being filled.
pub(super) enum Src<'a> {
    Playlist { pl: &'a Playlist, cursor: Cursor },
    Media { items: &'a [String], pos: usize },
    Category { category: &'a str, kind: MediaKind },
    AdBreak { brk: &'a AdBreak },
    Empty,
}

/// Owned match context (so it can outlive borrows of the planner).
pub(super) struct MatchCtxOwned {
    pub(super) at: WallTime,
    pub(super) category: Option<String>,
    pub(super) playlist: Option<String>,
    pub(super) block: Option<String>,
    pub(super) media: Option<String>,
    pub(super) kind: Option<MediaKind>,
}

impl MatchCtxOwned {
    pub(super) fn as_ctx(&self) -> MatchCtx<'_> {
        MatchCtx {
            at: Some(self.at),
            category: self.category.as_deref(),
            playlist: self.playlist.as_deref(),
            block: self.block.as_deref(),
            media: self.media.as_deref(),
            kind: self.kind,
        }
    }
}

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    pub(super) fn warn(&mut self, kind: WarningKind, at: Option<WallTime>, msg: impl Into<String>) {
        self.warnings.push(PlanWarning { kind, at, message: msg.into() });
    }

    pub(super) fn uid(&mut self) -> String {
        self.seq += 1;
        format!("{}-{:05}", self.uid_base, self.seq)
    }

    pub(super) fn playlist(&self, id: &str) -> Option<&'a Playlist> {
        self.inp.playlists.iter().find(|p| p.id == id)
    }

    pub(super) fn media(&self, id: &str) -> Option<&'a MediaItem> {
        self.inp.library.get(id)
    }
}

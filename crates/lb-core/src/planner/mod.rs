//! The broadcast planner: expands the grid (or a sequential playlist) into a
//! timed [`Rundown`], inserting jingles, ads and promos according to rules,
//! filling gaps and honouring fixed start times.
//!
//! Guarantees:
//! * Content is never cut unless the block sets `allow_truncate` (or the
//!   media itself defines in/out points).
//! * Results are deterministic for the same inputs and seed.
//! * When the next block has a fixed start, the planner first drops optional
//!   interstitials, then truncates (if allowed), and only then delays the
//!   fixed block, reporting the delay as a warning.
//!
//! Module layout (all stages are `impl Planner` blocks):
//! * [`context`] — planner working state and basic helpers
//! * [`grid`] — walking block instances (grid / sequential modes)
//! * [`window`] — fitting content between fixed start times
//! * [`block`] — filling one block with main content
//! * [`source`] — block content sources and weighted picking
//! * [`rules`] — interstitial rules and media selection
//! * [`ads`], [`promos`], [`gap`] — ad breaks, promos, gap fillers
//! * [`items`] — rundown item constructors
//! * [`transitions`] — fades applied after planning

mod ads;
mod block;
mod context;
mod gap;
mod grid;
mod items;
mod promos;
mod rules;
mod source;
mod transitions;
mod window;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::history::{Overlay, PlayHistory};
use crate::media::Library;
use crate::playlist::{Cursor, Playlist};
use crate::rng::Rng;
use crate::rules::RuleSet;
use crate::rundown::{Rundown, RundownItem};
use crate::schedule::{Block, BlockSource, ScheduleSet};
use crate::{Ms, WallTime};

use context::Planner;

/// Minimum remaining play time for a truncated item to be worth starting.
pub(crate) const MIN_TRUNCATED_PLAY_MS: Ms = 10_000;
/// Safety cap on items per plan.
pub(crate) const MAX_ITEMS: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PlanMode {
    /// Follow the broadcast grid with fixed start times.
    #[default]
    Grid,
    /// Play a playlist continuously; rules still apply, no fixed times.
    Sequential { playlist: String },
}

/// Static inputs for a planning pass.
pub struct PlanInput<'a> {
    pub library: &'a Library,
    pub playlists: &'a [Playlist],
    pub schedules: &'a ScheduleSet,
    pub rules: &'a RuleSet,
    pub history: &'a dyn PlayHistory,
    pub mode: &'a PlanMode,
    pub seed: u64,
    /// Operator-selected schedule (overrides activation times).
    pub force_schedule: Option<&'a str>,
}

/// Mutable planning state that the engine persists between passes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanState {
    #[serde(default)]
    pub cursors: HashMap<String, Cursor>,
    /// Last media picked per rule / ad break (sequential rotation, no-repeat).
    #[serde(default)]
    pub last_pick: HashMap<String, String>,
    /// Last time each rule fired.
    #[serde(default)]
    pub rule_last_fired: HashMap<String, WallTime>,
}

impl PlanState {
    /// Advance state as if `item` aired (engine calls this on start).
    pub fn commit(&mut self, item: &RundownItem) {
        if let Some((pl, c)) = &item.cursor_after {
            self.cursors.insert(pl.clone(), *c);
        }
        if let Some(r) = &item.rule {
            if !item.media.is_empty() {
                self.last_pick.insert(r.clone(), item.media.clone());
            }
            self.rule_last_fired.insert(r.clone(), item.start);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanRequest {
    /// When the first planned item starts.
    pub from: WallTime,
    /// Plan at least until this moment.
    pub until: WallTime,
    /// Continue inside this block instance (`date:block_id`).
    pub current_block: Option<String>,
    /// Main items already aired in the current block.
    pub block_items_done: usize,
}

impl PlanRequest {
    pub fn new(from: WallTime, until: WallTime) -> Self {
        PlanRequest { from, until, current_block: None, block_items_done: 0 }
    }
}

/// Plan a rundown for `[from, until)`.
pub fn plan(input: &PlanInput, state: &PlanState, from: WallTime, until: WallTime) -> Rundown {
    plan_req(input, state, &PlanRequest::new(from, until))
}

/// Plan with full control over continuation.
pub fn plan_req(input: &PlanInput, state: &PlanState, req: &PlanRequest) -> Rundown {
    // Sequential mode is modelled as one endless follow block.
    let seq_block = match input.mode {
        PlanMode::Sequential { playlist } => {
            Some(Block::new("main", None, BlockSource::Playlist { playlist: playlist.clone(), restart: false }))
        }
        PlanMode::Grid => None,
    };
    let mut p = Planner {
        inp: input,
        st: state.clone(),
        hist: Overlay::new(input.history),
        rng: Rng::derive(input.seed, &[&req.from.to_string()]),
        out: Vec::new(),
        warnings: Vec::new(),
        seq: 0,
        uid_base: req.from.format("%Y%m%d%H%M%S").to_string(),
    };
    match (input.mode, seq_block.as_ref()) {
        (PlanMode::Sequential { playlist }, Some(b)) => p.plan_sequential(playlist, b, req),
        _ => p.plan_grid(req),
    }
    let mut r = Rundown { items: p.out, warnings: p.warnings };
    transitions::apply_transitions(&mut r, input.rules);
    r
}

#[cfg(test)]
mod tests;

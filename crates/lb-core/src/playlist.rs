//! Playlists and deterministic playback order.

use serde::{Deserialize, Serialize};

use crate::Ms;
use crate::media::Library;
use crate::rng::Rng;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaylistEntry {
    pub media: String,
    /// Keep this entry at its index when the playlist is shuffled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// Entry disabled without removing it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

impl PlaylistEntry {
    pub fn new(media: impl Into<String>) -> Self {
        PlaylistEntry { media: media.into(), pinned: false, disabled: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Playlist {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub entries: Vec<PlaylistEntry>,
    /// Restart from the beginning after the last entry.
    #[serde(default = "yes", rename = "loop")]
    pub looped: bool,
    /// Random order (reshuffled every cycle, pinned entries stay in place).
    #[serde(default)]
    pub shuffle: bool,
    /// Default category for entries when the media has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

fn yes() -> bool {
    true
}

impl Playlist {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Playlist { name: id.clone(), id, entries: vec![], looped: true, shuffle: false, category: None }
    }

    pub fn with_media(mut self, ids: &[&str]) -> Self {
        self.entries.extend(ids.iter().map(|m| PlaylistEntry::new(*m)));
        self
    }

    /// Playback order of entry indexes for a given cycle.
    pub fn order(&self, seed: u64, cycle: u64) -> Vec<usize> {
        let active: Vec<usize> = (0..self.entries.len()).filter(|i| !self.entries[*i].disabled).collect();
        if !self.shuffle {
            return active;
        }
        let pinned: Vec<(usize, usize)> =
            active.iter().enumerate().filter(|(_, i)| self.entries[**i].pinned).map(|(pos, i)| (pos, *i)).collect();
        let mut free: Vec<usize> = active.iter().copied().filter(|i| !self.entries[*i].pinned).collect();
        let mut rng = Rng::derive(seed, &[&self.id, &cycle.to_string()]);
        rng.shuffle(&mut free);
        let mut out = Vec::with_capacity(active.len());
        let mut free_it = free.into_iter();
        let mut pin_it = pinned.into_iter().peekable();
        for pos in 0..active.len() {
            if pin_it.peek().is_some_and(|(p, _)| *p == pos) {
                out.push(pin_it.next().unwrap().1);
            } else if let Some(f) = free_it.next() {
                out.push(f);
            }
        }
        out
    }

    /// Total duration of enabled entries.
    pub fn total_duration(&self, lib: &Library) -> Ms {
        self.entries.iter().filter(|e| !e.disabled).filter_map(|e| lib.get(&e.media)).map(|m| m.play_duration()).sum()
    }

    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.entries.len() || to >= self.entries.len() {
            return false;
        }
        let e = self.entries.remove(from);
        self.entries.insert(to, e);
        true
    }
}

/// Position inside a playlist; persisted by the engine so series continue
/// where they stopped.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Cursor {
    /// Position within the cycle order.
    pub pos: usize,
    /// How many full cycles were completed (drives reshuffling).
    pub cycle: u64,
}

impl Cursor {
    /// Return the next entry index and advance; `None` when a non-looping
    /// playlist is exhausted.
    pub fn next(&mut self, pl: &Playlist, seed: u64) -> Option<usize> {
        let order = pl.order(seed, self.cycle);
        if order.is_empty() {
            return None;
        }
        if self.pos >= order.len() {
            if !pl.looped {
                return None;
            }
            self.pos = 0;
            self.cycle += 1;
            return self.next(pl, seed);
        }
        let idx = order[self.pos];
        self.pos += 1;
        Some(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_and_loop() {
        let pl = Playlist::new("p").with_media(&["a", "b", "c"]);
        let mut c = Cursor::default();
        let got: Vec<_> = (0..5).map(|_| pl.entries[c.next(&pl, 1).unwrap()].media.clone()).collect();
        assert_eq!(got, ["a", "b", "c", "a", "b"]);
        assert_eq!(c.cycle, 1);
    }

    #[test]
    fn no_loop_exhausts() {
        let mut pl = Playlist::new("p").with_media(&["a"]);
        pl.looped = false;
        let mut c = Cursor::default();
        assert!(c.next(&pl, 1).is_some());
        assert!(c.next(&pl, 1).is_none());
    }

    #[test]
    fn shuffle_keeps_pins_and_is_deterministic() {
        let mut pl = Playlist::new("p").with_media(&["a", "b", "c", "d", "e", "f"]);
        pl.shuffle = true;
        pl.entries[0].pinned = true;
        pl.entries[3].pinned = true;
        for cycle in 0..10 {
            let o = pl.order(42, cycle);
            assert_eq!(o.len(), 6);
            assert_eq!(o[0], 0);
            assert_eq!(o[3], 3);
            assert_eq!(o, pl.order(42, cycle));
        }
        let distinct: std::collections::HashSet<_> = (0..10).map(|c| pl.order(42, c)).collect();
        assert!(distinct.len() > 1, "cycles should reshuffle");
    }

    #[test]
    fn move_entries() {
        let mut pl = Playlist::new("p").with_media(&["a", "b", "c"]);
        assert!(pl.move_entry(0, 2));
        let ids: Vec<_> = pl.entries.iter().map(|e| e.media.as_str()).collect();
        assert_eq!(ids, ["b", "c", "a"]);
    }
}

//! Snapshot-based undo / redo for document drafts (playlists, grid,
//! rules, graphics). Every edit pushes the previous state.

const DEPTH: usize = 100;

#[derive(Debug, Clone)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        History { undo: Vec::new(), redo: Vec::new() }
    }
}

impl<T: Clone + PartialEq> History<T> {
    /// Record `before` (the state prior to an edit). Equal states are ignored.
    pub fn record(&mut self, before: &T, after: &T) {
        if before == after {
            return;
        }
        self.undo.push(before.clone());
        if self.undo.len() > DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Step back: returns the state to restore.
    pub fn undo(&mut self, current: &T) -> Option<T> {
        let prev = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(prev)
    }

    pub fn redo(&mut self, current: &T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_roundtrip() {
        let mut h = History::default();
        let (a, b, c) = (vec![1], vec![1, 2], vec![1, 2, 3]);
        h.record(&a, &b);
        h.record(&b, &c);
        assert_eq!(h.undo(&c), Some(b.clone()));
        assert_eq!(h.undo(&b), Some(a.clone()));
        assert_eq!(h.undo(&a), None);
        assert_eq!(h.redo(&a), Some(b.clone()));
        // a new edit drops the redo branch
        h.record(&b, &vec![9]);
        assert!(!h.can_redo());
    }

    #[test]
    fn identical_states_are_not_recorded() {
        let mut h = History::default();
        h.record(&1, &1);
        assert!(!h.can_undo());
    }
}
